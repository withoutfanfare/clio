# Clio Operational Roadmap

This is the canonical register for unfinished deployment, installation and
infrastructure work. The [deployment runbook](deployment.md) explains how to
operate the current system; this file records what still needs to change.

## Keeping the roadmap current

- Review it after every Atlas deployment and on the first working day of each
  month.
- Use only `Planned`, `In progress`, `Blocked` or `Done` as statuses. A blocked
  item must state the condition that will unblock it.
- When work starts, add its issue or pull request link to the status cell and
  keep the roadmap ID in that issue or pull request.
- Mark an item `Done` only when its "Finished when" evidence has been checked.
  Move completed rows to a `Completed` section rather than deleting them.
- Update the corresponding importance-5 Clio project memory after changing
  this file. Git remains the source of truth if the two disagree.

Priority 1 is the next resilience or usability work. Priority 2 becomes useful
as releases, hosts or operational reliance increase.

## Open work

| ID | Priority | Status | Work | Trigger or next action | Finished when |
|---|---:|---|---|---|---|
| CLIO-OPS-001 | 1 | Planned | Off-host Atlas backups | Implement before relying on Atlas for disaster recovery. | Encrypted standalone SQLite snapshots leave Atlas automatically; retention is enforced; failures alert; and a restore drill passes. |
| CLIO-OPS-002 | 1 | Blocked | Install the current release on the Mac mini and MacBook Pro | Resume as soon as each machine is online and reachable by SSH. | Each machine checks out the same pushed SHA, runs `macos-install.sh`, reconnects its AI clients, and passes cross-machine write, recall and namespace checks. |
| CLIO-APP-001 | 1 | In progress | Persist one Atlas route for CLI, hooks, MCP clients and Tauri | Deploy the implementation and verify a Finder launch against Atlas. | Atlas settings persist without credentials, CLI and hooks forward to Atlas, MCP setup generates the bridge, Tauri connects from Finder with a useful failure state, and the local daemon is disabled. |
| CLIO-OPS-003 | 2 | Planned | Manage Atlas infrastructure as code | Implement before rebuilding Atlas or adding another server. | A private Ansible setup recreates the user, packages, Rust toolchain, SSH and firewall policy, ONNX Runtime and backup job without committing secrets; a rebuild is proven. |
| CLIO-REL-001 | 2 | Planned | Publish release artefacts from CI | Implement when releases become frequent or more than a few Macs need updates. | CI publishes checksummed Linux `x86_64`, macOS `arm64` and macOS `x86_64` builds; the Mac app is Developer ID signed, notarised and stapled; Atlas migration remains an explicit operator step. |
| CLIO-OPS-004 | 2 | Planned | Add operational monitoring | Implement before Clio becomes business-critical. | Disk capacity, backup age, SQLite integrity and deployment failures are monitored; alerts are actionable and have been tested. |

## Completed

Move finished rows here with the completion date and links to the evidence.
