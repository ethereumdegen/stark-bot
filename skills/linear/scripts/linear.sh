#!/usr/bin/env bash
# Linear CLI for Starkbot — talks to Linear GraphQL API
# Usage: linear.sh <action> [json_args]
set -euo pipefail

ACTION="${1:-help}"
ARGS="${2:-\{\}}"

API="https://api.linear.app/graphql"

# Catch any unexpected failures and print context
trap 'echo "ERROR: linear.sh failed at line $LINENO (action=$ACTION)" >&2' ERR

if [[ -z "${LINEAR_API_KEY:-}" ]]; then
  echo "ERROR: LINEAR_API_KEY is not set. Get one at https://linear.app/settings/api"
  exit 1
fi

# ── helpers ──────────────────────────────────────────────────────────────────

# Execute a GraphQL query. Takes query string, optional variables JSON object.
# Uses jq to build the JSON payload so escaping is always correct.
gql() {
  local query="$1"
  local variables="${2:-null}"

  local payload
  payload=$(jq -n --arg q "$query" --argjson v "$variables" '{query: $q, variables: $v}')

  local http_code response
  local tmpfile
  tmpfile=$(mktemp)
  http_code=$(curl -sS -o "$tmpfile" -w '%{http_code}' -X POST "$API" \
    -H "Content-Type: application/json" \
    -H "Authorization: $LINEAR_API_KEY" \
    -d "$payload" 2>&1) || {
    echo "ERROR: Linear API request failed (curl error)"
    cat "$tmpfile" 2>/dev/null
    rm -f "$tmpfile"
    exit 1
  }
  response=$(cat "$tmpfile")
  rm -f "$tmpfile"

  if [[ "$http_code" -ge 400 ]]; then
    echo "ERROR: Linear API returned HTTP $http_code"
    echo "$response" | jq -r '.' 2>/dev/null || echo "$response"
    exit 1
  fi

  # Check for GraphQL errors
  local has_errors
  has_errors=$(echo "$response" | jq -r 'if .errors then "yes" else "no" end' 2>/dev/null || echo "no")
  if [[ "$has_errors" == "yes" ]]; then
    echo "ERROR: GraphQL error"
    echo "$response" | jq -r '.errors[0].message // .errors[0] // "Unknown error"'
    echo "Query: $(echo "$payload" | jq -r '.query[:120]')" >&2
    exit 1
  fi

  echo "$response"
}

arg() {
  echo "$ARGS" | jq -r ".${1} // empty"
}

# ── commands ─────────────────────────────────────────────────────────────────

cmd_teams() {
  local resp
  resp=$(gql '{ teams { nodes { id key name } } }')
  echo "$resp" | jq -r '.data.teams.nodes[] | "\(.key)\t\(.name)\t\(.id)"' | column -t -s$'\t'
}

cmd_my_issues() {
  local resp
  resp=$(gql '{ viewer { assignedIssues(orderBy: updatedAt, first: 50, filter: { state: { type: { nin: ["completed","canceled"] } } }) { nodes { identifier title priority state { name } project { name } } } } }')
  echo "$resp" | jq -r '.data.viewer.assignedIssues.nodes[] | "\(.identifier)\t[\(.state.name)]\tP\(.priority)\t\(.title)\t\(.project.name // "-")"' | column -t -s$'\t'
}

cmd_my_todos() {
  local resp
  resp=$(gql '{ viewer { assignedIssues(orderBy: updatedAt, first: 50, filter: { state: { name: { in: ["Todo","Backlog"] } } }) { nodes { identifier title priority state { name } } } } }')
  echo "$resp" | jq -r '.data.viewer.assignedIssues.nodes[] | "\(.identifier)\t[\(.state.name)]\tP\(.priority)\t\(.title)"' | column -t -s$'\t'
}

cmd_urgent() {
  local resp
  resp=$(gql '{ issues(orderBy: updatedAt, first: 50, filter: { priority: { in: [1,2] }, state: { type: { nin: ["completed","canceled"] } } }) { nodes { identifier title priority assignee { name } state { name } } } }')
  echo "$resp" | jq -r '.data.issues.nodes[] | "\(.identifier)\tP\(.priority)\t[\(.state.name)]\t\(.assignee.name // "unassigned")\t\(.title)"' | column -t -s$'\t'
}

cmd_team() {
  local team
  team=$(arg "team")
  if [[ -z "$team" ]]; then
    team="${LINEAR_DEFAULT_TEAM:-}"
    if [[ -z "$team" ]]; then
      echo "ERROR: team key required. Use {\"team\":\"TEAM_KEY\"} or set LINEAR_DEFAULT_TEAM"
      exit 1
    fi
  fi
  local vars
  vars=$(jq -n --arg t "$team" '{teamKey: $t}')
  local resp
  resp=$(gql 'query($teamKey: String!) { teams(filter: { key: { eq: $teamKey } }) { nodes { issues(orderBy: updatedAt, first: 50, filter: { state: { type: { nin: ["completed","canceled"] } } }) { nodes { identifier title priority assignee { name } state { name } } } } } }' "$vars")
  echo "$resp" | jq -r '.data.teams.nodes[0].issues.nodes[] | "\(.identifier)\t[\(.state.name)]\tP\(.priority)\t\(.assignee.name // "unassigned")\t\(.title)"' | column -t -s$'\t'
}

cmd_project() {
  local name
  name=$(arg "name")
  if [[ -z "$name" ]]; then
    echo "ERROR: project name required. Use {\"name\":\"Project Name\"}"
    exit 1
  fi
  local vars
  vars=$(jq -n --arg n "$name" '{name: $n}')
  local resp
  resp=$(gql 'query($name: String!) { projects(filter: { name: { containsIgnoreCase: $name } }, first: 1) { nodes { name issues(orderBy: updatedAt, first: 100) { nodes { identifier title priority assignee { name } state { name } } } } } }' "$vars")
  local project_name
  project_name=$(echo "$resp" | jq -r '.data.projects.nodes[0].name // "Not found"')
  echo "Project: $project_name"
  echo "---"
  echo "$resp" | jq -r '.data.projects.nodes[0].issues.nodes[] // empty | "\(.identifier)\t[\(.state.name)]\tP\(.priority)\t\(.assignee.name // "unassigned")\t\(.title)"' | column -t -s$'\t'
}

cmd_issue() {
  local id
  id=$(arg "id")
  if [[ -z "$id" ]]; then
    echo "ERROR: issue identifier required. Use {\"id\":\"TEAM-123\"}"
    exit 1
  fi
  local vars
  vars=$(jq -n --arg id "$id" '{id: $id}')
  local resp
  resp=$(gql 'query($id: String!) { issue(id: $id) { identifier title description priority priorityLabel state { name } assignee { name } team { key name } project { name } labels { nodes { name } } createdAt updatedAt comments { nodes { body createdAt user { name } } } } }' "$vars")
  echo "$resp" | jq -r '
    .data.issue |
    "[\(.identifier)] \(.title)",
    "Status: \(.state.name)  Priority: \(.priorityLabel)  Assignee: \(.assignee.name // "unassigned")",
    "Team: \(.team.key) (\(.team.name))  Project: \(.project.name // "-")",
    "Labels: \([ .labels.nodes[].name ] | join(", ") // "-")",
    "Created: \(.createdAt[:10])  Updated: \(.updatedAt[:10])",
    "",
    (.description // "(no description)"),
    "",
    if (.comments.nodes | length) > 0 then
      "--- Comments ---",
      (.comments.nodes[] | "\(.user.name) (\(.createdAt[:10])): \(.body)")
    else "No comments" end'
}

cmd_branch() {
  local id
  id=$(arg "id")
  if [[ -z "$id" ]]; then
    echo "ERROR: issue identifier required. Use {\"id\":\"TEAM-123\"}"
    exit 1
  fi
  local vars
  vars=$(jq -n --arg id "$id" '{id: $id}')
  local resp
  resp=$(gql 'query($id: String!) { issue(id: $id) { branchName } }' "$vars")
  echo "$resp" | jq -r '.data.issue.branchName'
}

cmd_create() {
  local team title description
  team=$(arg "team")
  title=$(arg "title")
  description=$(arg "description")

  if [[ -z "$team" ]]; then
    team="${LINEAR_DEFAULT_TEAM:-}"
  fi
  if [[ -z "$team" ]]; then
    echo "ERROR: team key required. Use {\"team\":\"TEAM_KEY\"} or set LINEAR_DEFAULT_TEAM"
    exit 1
  fi
  if [[ -z "$title" ]]; then
    echo "ERROR: title required. Use {\"title\":\"Issue title\"}"
    exit 1
  fi

  # Resolve team key to team ID
  local team_vars
  team_vars=$(jq -n --arg t "$team" '{teamKey: $t}')
  local team_resp
  team_resp=$(gql 'query($teamKey: String!) { teams(filter: { key: { eq: $teamKey } }) { nodes { id } } }' "$team_vars")
  local team_id
  team_id=$(echo "$team_resp" | jq -r '.data.teams.nodes[0].id // empty')
  if [[ -z "$team_id" ]]; then
    echo "ERROR: Team '$team' not found"
    exit 1
  fi

  local vars resp
  if [[ -n "$description" ]]; then
    vars=$(jq -n --arg tid "$team_id" --arg t "$title" --arg d "$description" \
      '{teamId: $tid, title: $t, description: $d}')
    resp=$(gql 'mutation($teamId: String!, $title: String!, $description: String!) { issueCreate(input: { teamId: $teamId, title: $title, description: $description }) { success issue { identifier title url } } }' "$vars")
  else
    vars=$(jq -n --arg tid "$team_id" --arg t "$title" \
      '{teamId: $tid, title: $t}')
    resp=$(gql 'mutation($teamId: String!, $title: String!) { issueCreate(input: { teamId: $teamId, title: $title }) { success issue { identifier title url } } }' "$vars")
  fi
  echo "$resp" | jq -r '.data.issueCreate.issue | "Created: \(.identifier) — \(.title)\nURL: \(.url)"'
}

cmd_comment() {
  local id body
  id=$(arg "id")
  body=$(arg "body")
  if [[ -z "$id" || -z "$body" ]]; then
    echo "ERROR: id and body required. Use {\"id\":\"TEAM-123\",\"body\":\"Comment text\"}"
    exit 1
  fi

  # Resolve issue identifier to issue UUID
  local id_vars
  id_vars=$(jq -n --arg id "$id" '{id: $id}')
  local issue_resp
  issue_resp=$(gql 'query($id: String!) { issue(id: $id) { id } }' "$id_vars")
  local issue_id
  issue_id=$(echo "$issue_resp" | jq -r '.data.issue.id // empty')
  if [[ -z "$issue_id" ]]; then
    echo "ERROR: Issue '$id' not found"
    exit 1
  fi

  local vars
  vars=$(jq -n --arg iid "$issue_id" --arg b "$body" '{issueId: $iid, body: $b}')
  local resp
  resp=$(gql 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success comment { id createdAt } } }' "$vars")
  echo "$resp" | jq -r '.data.commentCreate | if .success then "Comment added successfully" else "Failed to add comment" end'
}

cmd_status() {
  local id status_name
  id=$(arg "id")
  status_name=$(arg "status")
  if [[ -z "$id" || -z "$status_name" ]]; then
    echo "ERROR: id and status required. Use {\"id\":\"TEAM-123\",\"status\":\"progress\"}"
    exit 1
  fi

  # Map friendly names to Linear state names
  local state_name
  case "$status_name" in
    todo)     state_name="Todo" ;;
    progress) state_name="In Progress" ;;
    review)   state_name="In Review" ;;
    done)     state_name="Done" ;;
    blocked)  state_name="Blocked" ;;
    *)        state_name="$status_name" ;;
  esac

  # Resolve issue to get UUID and team ID
  local id_vars
  id_vars=$(jq -n --arg id "$id" '{id: $id}')
  local issue_resp
  issue_resp=$(gql 'query($id: String!) { issue(id: $id) { id team { id } } }' "$id_vars")
  local issue_id team_id
  issue_id=$(echo "$issue_resp" | jq -r '.data.issue.id // empty')
  team_id=$(echo "$issue_resp" | jq -r '.data.issue.team.id // empty')
  if [[ -z "$issue_id" ]]; then
    echo "ERROR: Issue '$id' not found"
    exit 1
  fi

  # Find matching workflow state for the team
  local state_vars
  state_vars=$(jq -n --arg tid "$team_id" --arg s "$state_name" '{teamId: $tid, stateName: $s}')
  local states_resp
  states_resp=$(gql 'query($teamId: ID!, $stateName: String!) { workflowStates(filter: { team: { id: { eq: $teamId } }, name: { containsIgnoreCase: $stateName } }) { nodes { id name } } }' "$state_vars")
  local state_id
  state_id=$(echo "$states_resp" | jq -r '.data.workflowStates.nodes[0].id // empty')
  if [[ -z "$state_id" ]]; then
    echo "ERROR: State '$state_name' not found for this team"
    exit 1
  fi

  local vars
  vars=$(jq -n --arg iid "$issue_id" --arg sid "$state_id" '{issueId: $iid, stateId: $sid}')
  local resp
  resp=$(gql 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }' "$vars")
  echo "$resp" | jq -r '.data.issueUpdate.issue | "\(.identifier) -> \(.state.name)"'
}

cmd_assign() {
  local id user
  id=$(arg "id")
  user=$(arg "user")
  if [[ -z "$id" || -z "$user" ]]; then
    echo "ERROR: id and user required. Use {\"id\":\"TEAM-123\",\"user\":\"userName\"}"
    exit 1
  fi

  # Resolve issue
  local id_vars
  id_vars=$(jq -n --arg id "$id" '{id: $id}')
  local issue_resp
  issue_resp=$(gql 'query($id: String!) { issue(id: $id) { id } }' "$id_vars")
  local issue_id
  issue_id=$(echo "$issue_resp" | jq -r '.data.issue.id // empty')
  if [[ -z "$issue_id" ]]; then
    echo "ERROR: Issue '$id' not found"
    exit 1
  fi

  # Find user by display name
  local user_vars
  user_vars=$(jq -n --arg u "$user" '{userName: $u}')
  local user_resp
  user_resp=$(gql 'query($userName: String!) { users(filter: { displayName: { containsIgnoreCase: $userName } }) { nodes { id name } } }' "$user_vars")
  local user_id
  user_id=$(echo "$user_resp" | jq -r '.data.users.nodes[0].id // empty')
  if [[ -z "$user_id" ]]; then
    echo "ERROR: User '$user' not found"
    exit 1
  fi

  local vars
  vars=$(jq -n --arg iid "$issue_id" --arg uid "$user_id" '{issueId: $iid, assigneeId: $uid}')
  local resp
  resp=$(gql 'mutation($issueId: String!, $assigneeId: String!) { issueUpdate(id: $issueId, input: { assigneeId: $assigneeId }) { success issue { identifier assignee { name } } } }' "$vars")
  echo "$resp" | jq -r '.data.issueUpdate.issue | "\(.identifier) -> assigned to \(.assignee.name)"'
}

cmd_priority() {
  local id priority_name
  id=$(arg "id")
  priority_name=$(arg "priority")
  if [[ -z "$id" || -z "$priority_name" ]]; then
    echo "ERROR: id and priority required. Use {\"id\":\"TEAM-123\",\"priority\":\"high\"}"
    exit 1
  fi

  local priority_val
  case "$priority_name" in
    none)   priority_val=0 ;;
    urgent) priority_val=1 ;;
    high)   priority_val=2 ;;
    medium) priority_val=3 ;;
    low)    priority_val=4 ;;
    *)
      echo "ERROR: Invalid priority '$priority_name'. Use: urgent, high, medium, low, none"
      exit 1
      ;;
  esac

  # Resolve issue
  local id_vars
  id_vars=$(jq -n --arg id "$id" '{id: $id}')
  local issue_resp
  issue_resp=$(gql 'query($id: String!) { issue(id: $id) { id } }' "$id_vars")
  local issue_id
  issue_id=$(echo "$issue_resp" | jq -r '.data.issue.id // empty')
  if [[ -z "$issue_id" ]]; then
    echo "ERROR: Issue '$id' not found"
    exit 1
  fi

  local vars
  vars=$(jq -n --arg iid "$issue_id" --argjson p "$priority_val" '{issueId: $iid, priority: $p}')
  local resp
  resp=$(gql 'mutation($issueId: String!, $priority: Int!) { issueUpdate(id: $issueId, input: { priority: $priority }) { success issue { identifier priorityLabel } } }' "$vars")
  echo "$resp" | jq -r '.data.issueUpdate.issue | "\(.identifier) -> priority: \(.priorityLabel)"'
}

cmd_standup() {
  echo "=== Daily Standup ==="
  echo ""

  echo "YOUR TODOS:"
  cmd_my_todos 2>/dev/null || echo "  (none)"
  echo ""

  echo "URGENT/HIGH PRIORITY:"
  cmd_urgent 2>/dev/null || echo "  (none)"
  echo ""

  echo "IN REVIEW:"
  local resp
  resp=$(gql '{ viewer { assignedIssues(first: 20, filter: { state: { name: { eq: "In Review" } } }) { nodes { identifier title } } } }')
  echo "$resp" | jq -r '.data.viewer.assignedIssues.nodes[] | "  \(.identifier)  \(.title)"' 2>/dev/null || echo "  (none)"
  echo ""

  echo "RECENTLY COMPLETED (last 7 days):"
  local since
  since=$(date -d '7 days ago' -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -v-7d -u +%Y-%m-%dT%H:%M:%SZ)
  local vars
  vars=$(jq -n --arg s "$since" '{since: $s}')
  local resp2
  resp2=$(gql 'query($since: DateTime!) { viewer { assignedIssues(first: 20, orderBy: updatedAt, filter: { state: { type: { eq: "completed" } }, updatedAt: { gte: $since } }) { nodes { identifier title completedAt } } } }' "$vars")
  echo "$resp2" | jq -r '.data.viewer.assignedIssues.nodes[] | "  \(.identifier)  \(.title)  (completed \(.completedAt[:10]))"' 2>/dev/null || echo "  (none)"
}

cmd_projects() {
  local resp
  resp=$(gql '{ projects(first: 50, orderBy: updatedAt) { nodes { name state progress teams { nodes { key } } lead { name } issues { nodes { id } } } } }')
  echo "$resp" | jq -r '.data.projects.nodes[] | "\(.name)\t\(.state)\t\((.progress * 100) | floor)%\t\(.lead.name // "-")\t\(.issues.nodes | length) issues\t\([.teams.nodes[].key] | join(","))"' | column -t -s$'\t'
}

cmd_help() {
  cat <<'HELP'
Linear CLI -- Commands:

  my-issues         Your assigned open issues
  my-todos          Your Todo/Backlog items
  urgent            Urgent/High priority across all teams

  teams             List available teams
  team              Issues for a team (args: team)
  project           Issues in a project (args: name)
  issue             Issue details + comments (args: id)
  branch            Git branch name for issue (args: id)

  create            Create issue (args: team, title, description?)
  comment           Add comment (args: id, body)
  status            Set status (args: id, status)
  assign            Assign issue (args: id, user)
  priority          Set priority (args: id, priority)

  standup           Daily standup summary
  projects          All projects with progress
HELP
}

# ── dispatch ─────────────────────────────────────────────────────────────────

case "$ACTION" in
  teams)      cmd_teams ;;
  my-issues)  cmd_my_issues ;;
  my-todos)   cmd_my_todos ;;
  urgent)     cmd_urgent ;;
  team)       cmd_team ;;
  project)    cmd_project ;;
  issue)      cmd_issue ;;
  branch)     cmd_branch ;;
  create)     cmd_create ;;
  comment)    cmd_comment ;;
  status)     cmd_status ;;
  assign)     cmd_assign ;;
  priority)   cmd_priority ;;
  standup)    cmd_standup ;;
  projects)   cmd_projects ;;
  help|*)     cmd_help ;;
esac
