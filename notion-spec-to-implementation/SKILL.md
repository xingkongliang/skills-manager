---
name: notion-spec-to-implementation
description: Turn Notion specs into implementation plans, tasks, and progress tracking.
---

# Spec to Implementation

Convert a Notion spec into linked implementation plans, tasks, and ongoing status updates.

## Quick Start
1. Locate the spec, then fetch it
2. Parse requirements and ambiguities
3. Create a plan page (pick template: quick vs. full)
4. Find task database, confirm schema, create tasks
5. Link spec ↔ plan ↔ tasks; keep status current

## Workflow

### 1) Locate and Read the Spec
- Search for the spec; if multiple hits, ask user which to use
- Fetch the page and scan for:
  - Requirements
  - Acceptance criteria
  - Constraints
  - Priorities
- Capture gaps/assumptions in a clarifications block before proceeding

### 2) Choose Plan Depth
- **Simple change** → quick implementation plan
- **Multi-phase feature/migration** → standard implementation plan

**Plan includes**:
- Overview
- Linked spec
- Requirements summary
- Phases
- Dependencies/risks
- Success criteria

### 3) Create Tasks
- Find the task database and confirm required properties
- Size tasks to 1-2 days
- Each task includes:
  - Context
  - Objective
  - Acceptance criteria
  - Dependencies
  - Resources

**Task properties**:
- Title with action verb
- Status
- Priority
- Relations to spec + plan
- Due date/story points/assignee (if provided)

### 4) Link Artifacts
- Plan links to spec
- Tasks link to both plan and spec
- Optionally update spec with "Implementation" section

### 5) Track Progress
- Post regular updates
- Close phases with milestone summaries
- Keep checklists and status fields in sync
- Note blockers and decisions

## Plan Templates

### Quick Implementation Plan
For simple changes:
- Single phase
- Few tasks
- Short timeline

### Standard Implementation Plan
For complex features:
- Multiple phases
- Dependencies mapped
- Risk assessment
- Rollback plan

## Task Structure

```
Task: [Action Verb] [Component/Feature]

Context:
- Why this task exists
- Related spec requirements

Objective:
- What "done" looks like

Acceptance Criteria:
- [ ] Specific, testable criteria
- [ ] Edge cases covered

Dependencies:
- Blocks: [tasks that block this]
- Blocked by: [tasks this depends on]

Resources:
- Design files
- Technical docs
- Related PRs
```