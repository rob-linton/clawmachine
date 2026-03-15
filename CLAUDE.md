# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## CRITICAL: WORK METHODOLOGY

### DO NOT ADD THIS COMMENT TO GIT COMMITS

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>

**These rules take absolute priority over all other considerations:**

### 1. Deep Thinking Over Speed
- **ALWAYS use maximum tokens** available for each request
- **NEVER rush to solutions** - thoroughness and accuracy are the top priority
- **Think deeply** about implications, edge cases, and system-wide impact before making changes
- Consider all related components that might be affected
- Review existing patterns and conventions in the codebase before implementing

### 2. Planning and Organization
- **ALWAYS create a detailed plan** before starting implementation, even when not explicitly in planning mode
- **ALWAYS use the TodoWrite tool** to create and maintain a task list for multi-step work
- Break down complex tasks into specific, actionable items
- **Show the todo list** at the start and update it as you progress
- Mark items as `in_progress` when starting, `completed` when finished
- If the plan is not fully implemented, say so and explain why

### 3. Verification and Quality Assurance
- **ALWAYS check your work** against the original plan when completed
- Verify that all requirements have been met
- Review all changes for consistency with codebase patterns
- Test critical paths mentally before declaring work complete
- Look for potential issues introduced by the changes

### 4. Communication and Transparency
- **ALWAYS provide maximum feedback** as you work through tasks
- Explain what you're doing and why at each step
- Share your reasoning for implementation decisions
- Call out any assumptions or uncertainties
- Describe what you're looking for when searching/reading code
- **Keep the todo list visible** and updated throughout the conversation

### 5. No Silent Work
- NEVER make changes without explaining them
- NEVER skip showing intermediate steps
- NEVER assume the user knows what you're thinking
- **Always narrate your process** so the user can follow along
