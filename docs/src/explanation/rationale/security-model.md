# Security model

This section explains the security posture of using Portal and Agent-box together or separately.

## Threat reduction goals

- Avoid broad host socket exposure to containers.
- Add policy gates to sensitive host operations.
- Provide audit-friendly mediation point.

## Trust boundaries

- **Container**: untrusted or semi-trusted agent execution context.
- **Portal host**: trusted broker enforcing method policy.
- **Host binaries** (`gh`, `wl-paste`): executed only by host broker.

## Control mechanisms

- Unix socket peer credentials for caller identity.
- Method-level policy modes (`allow/ask/deny`, `gh_exec` policy modes).
- Optional prompt-based approval command.
- Concurrency limits, rate limits, and request/prompt timeouts.
- Clipboard MIME allowlist and payload size limit.

## Residual risks

- Misconfigured policy can allow broader access than intended.
- Prompt UX failures can degrade safety or usability.
- Host broker is a sensitive component and must be monitored and updated.

## Operational guidance

- Keep policies explicit and minimal.
- Use deny-by-default for high-risk methods where appropriate.
- Enable debug logs during rollout and review outcomes.
