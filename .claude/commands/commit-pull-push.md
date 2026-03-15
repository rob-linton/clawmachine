---
allowed-tools: Bash(git status:*), Bash(git add:*), Bash(git commit:*), Bash(git stash:*), Bash(git pull:*), Bash(git push:*), Bash(git submodule:*), Bash(cd * && git:*), Bash(./installer/docker/scripts/sync-deploy-repo.sh:*)
description: Commit, pull, then push changes to remote
argument-hint: [commit message]
---

# Commit, Pull, and Push

Perform a git commit, pull, and push in sequence, handling submodules correctly and syncing the deployment repo.

## Instructions

### Step 1: Check for submodules
Run `git submodule status` to identify any submodules in the repo.

### Step 2: Handle submodule changes FIRST
For each submodule that has uncommitted changes or new commits:
1. `cd` into the submodule directory
2. Ensure you're on a branch (not detached HEAD): `git checkout main` or appropriate branch
3. Run `git status` to see changes
4. If there are changes to commit:
   - Stage changes: `git add -A`
   - Commit with the provided message (or generate one)
5. Push the submodule: `git push`
6. Return to parent directory

**Critical:** Submodules MUST be pushed before the parent repo, otherwise others will get references to commits that don't exist on the remote.

### Step 3: Handle parent repo (commit only)
1. Run `git status` in the parent repo to see all changes (including submodule pointer updates)
2. If there are changes to commit:
   - Stage all changes with `git add -A` (this includes updated submodule pointers)
   - Commit with the provided message (or generate one if not provided)
3. Stash any unstaged changes if needed: `git stash` (to allow rebase)
4. Pull with rebase to sync latest changes: `git pull --rebase origin main`
5. Pop stash if used: `git stash pop` (ignore errors if stash was empty)

**Do NOT push yet** - we need to sync the deployment repo first.

### Step 4: Sync and push deployment repo
After committing parent repo changes but BEFORE pushing:
1. Run `./installer/docker/scripts/sync-deploy-repo.sh ~/src/minnowvpn` to sync Docker deployment files
2. If the sync script reports changes in ~/src/minnowvpn:
   - `cd ~/src/minnowvpn`
   - Stage changes: `git add -A`
   - Commit: `git commit -m "Sync from minnowvpn-src"`
   - Pull with rebase: `git pull --rebase origin main`
   - Push: `git push`
   - Return to parent directory
3. If sync script fails (e.g., deployment repo doesn't exist locally), warn but continue

### Step 5: Push parent repo
1. Push to remote: `git push`

**Important:** Always perform the pull step even if there are no local changes to commit. This ensures you stay in sync with remote.

If any step fails, stop and report the error clearly.

The commit message is: $ARGUMENTS
