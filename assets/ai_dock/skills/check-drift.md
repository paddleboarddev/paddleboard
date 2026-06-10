---
description: Run script/check-upstream-drift to see how far PaddleBoard has diverged from upstream Zed
allowed-tools: ["Bash", "Read"]
---

# /check-drift — Upstream drift report

Run the upstream drift checker to see how far PaddleBoard has diverged from `zed-industries/zed`.

## Steps

1. Make sure the `zed` remote is configured:
   ```
   git remote get-url zed 2>/dev/null || echo "no zed remote"
   ```
   If missing, add it: `git remote add zed https://github.com/zed-industries/zed.git`

2. Fetch the latest upstream: `git fetch zed main`

3. Run the drift script: `./script/check-upstream-drift`

4. Summarize the results for the user:
   - Total lines changed vs upstream
   - Which crates have the most drift
   - How many `// PaddleBoard:` markers exist (`git grep -c "// PaddleBoard:"`)
   - When the last upstream merge happened (`git log --oneline --grep="Merge upstream" -1`)

## Notes

- This is a read-only operation; it doesn't change any files.
- If the script doesn't exist or fails, fall back to `git diff --stat zed/main...HEAD | tail -20` for a rough picture.

User input: $ARGUMENTS
