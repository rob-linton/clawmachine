use claw_models::{Job, Skill, SkillType, Workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Tracks everything the environment setup did, so teardown can undo it.
pub struct PreparedEnvironment {
    pub working_dir: PathBuf,
    pub is_temp: bool,
    pub claude_md_backup: Option<PathBuf>,
    pub original_claude_md: Option<String>,
    pub marker_file: Option<PathBuf>,
    pub deployed_skill_dirs: Vec<PathBuf>,
    pub pre_existing_skill_dirs: Vec<String>,
}

/// Skills harvested after execution.
pub struct HarvestedSkills {
    pub new_skills: Vec<Skill>,
    pub modified_claude_md: Option<String>,
}

/// Prepare a workspace for job execution.
/// Creates temp dir if needed, injects CLAUDE.md, deploys script skills.
pub async fn prepare_environment(
    job: &Job,
    workspace: Option<&Workspace>,
    skills: &[Skill],
) -> Result<PreparedEnvironment, String> {
    // 1. Resolve workspace directory
    let (working_dir, is_temp) = resolve_working_dir(job, workspace).await?;

    // 2. Snapshot pre-existing skill dirs
    let pre_existing_skill_dirs = snapshot_existing_skills(&working_dir).await;

    // 3. CLAUDE.md injection
    let (claude_md_backup, original_claude_md, marker_file) =
        inject_claude_md(job.id, &working_dir, workspace, skills).await?;

    // 4. Deploy script skills
    let deployed_skill_dirs = deploy_script_skills(&working_dir, skills).await?;

    Ok(PreparedEnvironment {
        working_dir,
        is_temp,
        claude_md_backup,
        original_claude_md,
        marker_file,
        deployed_skill_dirs,
        pre_existing_skill_dirs,
    })
}

/// Harvest new skills and CLAUDE.md changes after execution.
pub async fn harvest_skills(env: &PreparedEnvironment) -> HarvestedSkills {
    let mut new_skills = Vec::new();
    let modified_claude_md;

    // Check for new skill directories
    let skills_dir = env.working_dir.join(".claude").join("skills");
    if skills_dir.is_dir() {
        if let Ok(mut entries) = tokio::fs::read_dir(&skills_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

                // Skip if we deployed it or it pre-existed
                if env.deployed_skill_dirs.iter().any(|d| d == &path) {
                    continue;
                }
                if env.pre_existing_skill_dirs.contains(&dir_name) {
                    continue;
                }

                // Read SKILL.md
                let skill_md_path = path.join("SKILL.md");
                if !skill_md_path.exists() {
                    continue;
                }

                if let Ok(content) = tokio::fs::read_to_string(&skill_md_path).await {
                    let (name, description, body) = parse_skill_md(&content);
                    let files = read_skill_files(&path, "SKILL.md").await;

                    new_skills.push(Skill {
                        id: dir_name.clone(),
                        name: name.unwrap_or_else(|| dir_name.clone()),
                        skill_type: SkillType::Script,
                        content: body,
                        description: description.unwrap_or_default(),
                        tags: vec!["harvested".into()],
                        files,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });

                    tracing::info!(skill_id = %dir_name, "Harvested new skill from workspace");
                }
            }
        }
    }

    // Check if CLAUDE.md was modified
    modified_claude_md = check_claude_md_changes(env).await;

    HarvestedSkills {
        new_skills,
        modified_claude_md,
    }
}

/// Clean up the environment after job execution.
pub async fn teardown_environment(env: &PreparedEnvironment) {
    // Restore CLAUDE.md
    if let Some(backup_path) = &env.claude_md_backup {
        let claude_md_path = env.working_dir.join("CLAUDE.md");
        if let Err(e) = tokio::fs::copy(backup_path, &claude_md_path).await {
            tracing::warn!(error = %e, "Failed to restore CLAUDE.md from backup");
        }
        tokio::fs::remove_file(backup_path).await.ok();
    } else if env.original_claude_md.is_none() {
        // No original existed — remove the one we created
        let claude_md_path = env.working_dir.join("CLAUDE.md");
        if claude_md_path.exists() {
            tokio::fs::remove_file(&claude_md_path).await.ok();
        }
    }

    // Remove deployed skill dirs
    for dir in &env.deployed_skill_dirs {
        if let Err(e) = tokio::fs::remove_dir_all(dir).await {
            tracing::warn!(path = %dir.display(), error = %e, "Failed to remove deployed skill dir");
        }
    }

    // Remove marker
    if let Some(marker) = &env.marker_file {
        tokio::fs::remove_file(marker).await.ok();
    }

    // Remove temp dir
    if env.is_temp {
        if let Err(e) = tokio::fs::remove_dir_all(&env.working_dir).await {
            tracing::warn!(path = %env.working_dir.display(), error = %e, "Failed to remove temp workspace");
        }
    }
}

/// Recover from previous unclean shutdowns.
pub async fn crash_recovery() {
    let tmp_dir = PathBuf::from("/tmp/claw-jobs");
    if !tmp_dir.exists() {
        return;
    }

    tracing::info!("Scanning for crash recovery...");

    if let Ok(mut entries) = tokio::fs::read_dir(&tmp_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                recover_workspace(&path).await;
                // Remove stale temp dirs
                tokio::fs::remove_dir_all(&path).await.ok();
            }
        }
    }
}

// --- Internal helpers ---

async fn resolve_working_dir(job: &Job, workspace: Option<&Workspace>) -> Result<(PathBuf, bool), String> {
    // Priority: workspace_id → workspace.path, working_dir → use that, else temp
    if let Some(ws) = workspace {
        if ws.path.exists() && ws.path.is_dir() {
            return Ok((ws.path.clone(), false));
        }
        // Create workspace dir if it doesn't exist
        tokio::fs::create_dir_all(&ws.path)
            .await
            .map_err(|e| format!("Failed to create workspace dir: {e}"))?;
        return Ok((ws.path.clone(), false));
    }

    let wd = &job.working_dir;
    if wd != &PathBuf::from(".") && wd.exists() && wd.is_dir() {
        return Ok((wd.clone(), false));
    }

    // Create temp dir
    let tmp = PathBuf::from("/tmp/claw-jobs").join(job.id.to_string());
    tokio::fs::create_dir_all(&tmp)
        .await
        .map_err(|e| format!("Failed to create temp workspace: {e}"))?;
    Ok((tmp, true))
}

async fn snapshot_existing_skills(working_dir: &Path) -> Vec<String> {
    let skills_dir = working_dir.join(".claude").join("skills");
    let mut dirs = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&skills_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    dirs.push(name.to_string());
                }
            }
        }
    }
    dirs
}

async fn inject_claude_md(
    job_id: Uuid,
    working_dir: &Path,
    workspace: Option<&Workspace>,
    skills: &[Skill],
) -> Result<(Option<PathBuf>, Option<String>, Option<PathBuf>), String> {
    let config_skills: Vec<&Skill> = skills
        .iter()
        .filter(|s| s.skill_type == SkillType::ClaudeConfig)
        .collect();

    let ws_claude_md = workspace.and_then(|ws| ws.claude_md.as_deref());

    // Nothing to inject if no config skills and no workspace CLAUDE.md
    if config_skills.is_empty() && ws_claude_md.is_none() {
        return Ok((None, None, None));
    }

    let claude_md_path = working_dir.join("CLAUDE.md");
    let claw_dir = working_dir.join(".claw");
    tokio::fs::create_dir_all(&claw_dir)
        .await
        .map_err(|e| format!("Failed to create .claw dir: {e}"))?;

    // Read and backup existing CLAUDE.md
    let original = if claude_md_path.exists() {
        let content = tokio::fs::read_to_string(&claude_md_path)
            .await
            .map_err(|e| format!("Failed to read CLAUDE.md: {e}"))?;
        let backup = claw_dir.join(format!("CLAUDE.md.backup.{}", job_id));
        tokio::fs::write(&backup, &content)
            .await
            .map_err(|e| format!("Failed to backup CLAUDE.md: {e}"))?;
        Some((content, backup))
    } else {
        None
    };

    // Build merged content
    let mut sections = Vec::new();
    if let Some(ws_md) = ws_claude_md {
        sections.push(ws_md.to_string());
    } else if let Some((ref orig, _)) = original {
        sections.push(orig.clone());
    }

    if !config_skills.is_empty() {
        sections.push("\n## ClaudeCodeClaw Injected Skills\n".to_string());
        for skill in &config_skills {
            sections.push(format!("### {}\n\n{}", skill.name, skill.content));
        }
    }

    let merged = sections.join("\n\n");
    tokio::fs::write(&claude_md_path, &merged)
        .await
        .map_err(|e| format!("Failed to write CLAUDE.md: {e}"))?;

    // Write marker
    let marker_path = claw_dir.join(format!("injected-{}", job_id));
    let marker_content = serde_json::json!({
        "job_id": job_id.to_string(),
        "had_original_claude_md": original.is_some(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    tokio::fs::write(&marker_path, marker_content.to_string())
        .await
        .map_err(|e| format!("Failed to write marker: {e}"))?;

    let (original_content, backup_path) = match original {
        Some((content, backup)) => (Some(content), Some(backup)),
        None => (None, None),
    };

    Ok((backup_path, original_content, Some(marker_path)))
}

async fn deploy_script_skills(working_dir: &Path, skills: &[Skill]) -> Result<Vec<PathBuf>, String> {
    let mut deployed = Vec::new();

    let script_skills: Vec<&Skill> = skills
        .iter()
        .filter(|s| s.skill_type == SkillType::Script)
        .collect();

    if script_skills.is_empty() {
        return Ok(deployed);
    }

    let skills_dir = working_dir.join(".claude").join("skills");
    tokio::fs::create_dir_all(&skills_dir)
        .await
        .map_err(|e| format!("Failed to create .claude/skills: {e}"))?;

    for skill in &script_skills {
        let skill_dir = skills_dir.join(&skill.id);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .map_err(|e| format!("Failed to create skill dir {}: {e}", skill.id))?;

        // Write SKILL.md with frontmatter
        let skill_md = format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}",
            skill.name, skill.description, skill.content
        );
        tokio::fs::write(skill_dir.join("SKILL.md"), &skill_md)
            .await
            .map_err(|e| format!("Failed to write SKILL.md for {}: {e}", skill.id))?;

        // Write bundled files
        for (rel_path, content) in &skill.files {
            let file_path = skill_dir.join(rel_path);
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            tokio::fs::write(&file_path, content)
                .await
                .map_err(|e| format!("Failed to write {}: {e}", rel_path))?;

            // chmod +x for scripts
            #[cfg(unix)]
            if rel_path.starts_with("scripts/") {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                    let mut perms = metadata.permissions();
                    perms.set_mode(perms.mode() | 0o111);
                    tokio::fs::set_permissions(&file_path, perms).await.ok();
                }
            }
        }

        deployed.push(skill_dir);
        tracing::debug!(skill_id = %skill.id, "Deployed script skill to workspace");
    }

    Ok(deployed)
}

/// Parse SKILL.md content — extract YAML frontmatter for name/description.
fn parse_skill_md(content: &str) -> (Option<String>, Option<String>, String) {
    if !content.starts_with("---") {
        return (None, None, content.to_string());
    }

    if let Some(end) = content[3..].find("---") {
        let frontmatter = &content[3..3 + end];
        let body = content[3 + end + 3..].trim_start().to_string();
        let mut name = None;
        let mut description = None;

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("name:") {
                name = Some(val.trim().to_string());
            } else if let Some(val) = line.strip_prefix("description:") {
                description = Some(val.trim().to_string());
            }
        }

        (name, description, body)
    } else {
        (None, None, content.to_string())
    }
}

/// Read all files in a skill directory (excluding a specific file).
async fn read_skill_files(skill_dir: &Path, exclude: &str) -> HashMap<String, String> {
    let mut files = HashMap::new();
    let mut stack = vec![skill_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let relative = path.strip_prefix(skill_dir).unwrap_or(&path);
                    let rel_str = relative.to_string_lossy().to_string();
                    if rel_str == exclude {
                        continue;
                    }
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        files.insert(rel_str, content);
                    }
                }
            }
        }
    }

    files
}

/// Check if CLAUDE.md was modified beyond our injection.
async fn check_claude_md_changes(env: &PreparedEnvironment) -> Option<String> {
    if env.claude_md_backup.is_none() && env.original_claude_md.is_none() {
        // We didn't inject anything, so check if a new CLAUDE.md was created
        let path = env.working_dir.join("CLAUDE.md");
        if path.exists() {
            return tokio::fs::read_to_string(&path).await.ok();
        }
        return None;
    }

    // We injected — check if it was further modified
    // (This is a best-effort check; we can't perfectly detect changes vs our injection)
    None
}

async fn recover_workspace(dir: &Path) {
    let claw_dir = dir.join(".claw");
    if !claw_dir.exists() {
        return;
    }

    if let Ok(mut entries) = tokio::fs::read_dir(&claw_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("injected-") {
                continue;
            }

            // Read marker
            if let Ok(marker_json) = tokio::fs::read_to_string(entry.path()).await {
                if let Ok(marker) = serde_json::from_str::<serde_json::Value>(&marker_json) {
                    let job_id = marker["job_id"].as_str().unwrap_or("");
                    let had_original = marker["had_original_claude_md"].as_bool().unwrap_or(false);

                    // Restore CLAUDE.md
                    let backup = claw_dir.join(format!("CLAUDE.md.backup.{}", job_id));
                    let claude_md = dir.join("CLAUDE.md");
                    if had_original && backup.exists() {
                        tokio::fs::copy(&backup, &claude_md).await.ok();
                        tokio::fs::remove_file(&backup).await.ok();
                    } else if !had_original {
                        tokio::fs::remove_file(&claude_md).await.ok();
                    }

                    tracing::info!(job_id, "Recovered workspace from crash");
                }
            }

            // Remove marker
            tokio::fs::remove_file(entry.path()).await.ok();
        }
    }
}
