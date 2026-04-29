# Review command user prompt templates

## UNCOMMITTED_PROMPT

Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.

## BASE_BRANCH_PROMPT

Review the code changes against the base branch '{{base_branch}}'. The merge base commit for this comparison is {{merge_base_sha}}. Run `git diff {{merge_base_sha}}` to inspect the changes relative to {{base_branch}}. Provide prioritized, actionable findings.

## BASE_BRANCH_PROMPT_BACKUP

Review the code changes against the base branch '{{branch}}'. Start by finding the merge diff between the current branch and {{branch}}'s upstream e.g. (`git merge-base HEAD "$(git rev-parse --abbrev-ref "{{branch}}@{upstream}")"`), then run `git diff` against that SHA to see what changes we would merge into the {{branch}} branch. Provide prioritized, actionable findings.

## COMMIT_PROMPT

Review the code changes introduced by commit {{sha}}. Provide prioritized, actionable findings.

## COMMIT_PROMPT_WITH_TITLE

Review the code changes introduced by commit {{sha}} ("{{title}}"). Provide prioritized, actionable findings.
