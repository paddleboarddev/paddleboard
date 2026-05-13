#!/usr/bin/env bash
# Stop hook: block stopping if files were modified this session but RECAPS.md
# was not updated to reflect those changes.
#
# Logic: scan the transcript for Edit/Write/NotebookEdit tool calls in order.
# Track the last index of an edit to RECAPS.md and the last index of an edit
# to any other file. If non-RECAPS edits happened *after* the last RECAPS
# edit (or RECAPS was never touched), emit decision:"block" with feedback.

set -uo pipefail

input=$(cat)
transcript_path=$(jq -r '.transcript_path // empty' <<< "$input")

if [ -z "$transcript_path" ] || [ ! -f "$transcript_path" ]; then
  exit 0
fi

edits_output=$(jq -r '
  select(.type == "assistant") |
  .message.content[]? |
  select(.type == "tool_use") |
  select(.name == "Edit" or .name == "Write" or .name == "NotebookEdit") |
  .input.file_path // empty
' "$transcript_path" 2>/dev/null)

last_recap_idx=-1
last_other_idx=-1
idx=0

if [ -n "$edits_output" ]; then
  while IFS= read -r path; do
    if [ -z "$path" ]; then
      idx=$((idx + 1))
      continue
    fi
    case "$path" in
      # Agent bookkeeping — not project work. Memory files and tmp scratch
      # shouldn't trigger the recap requirement.
      */.claude/projects/*/memory/*|/tmp/*|/var/folders/*/T/*)
        ;;
      *)
        if [ "$(basename "$path")" = "RECAPS.md" ]; then
          last_recap_idx=$idx
        else
          last_other_idx=$idx
        fi
        ;;
    esac
    idx=$((idx + 1))
  done <<< "$edits_output"
fi

if [ "$last_other_idx" -ge 0 ] && [ "$last_recap_idx" -lt "$last_other_idx" ]; then
  jq -n '{
    decision: "block",
    reason: "RECAPS.md was not updated to reflect file changes made in this session. Before stopping: append a dated H3 section to RECAPS.md at the repo root (one H2 per day, newest-first; H3 per work unit with a few bullets covering what landed, what was intentionally preserved, and any follow-ups). See FORK_HYGIENE.md / the recaps-to-md feedback memory for the convention."
  }'
fi

exit 0
