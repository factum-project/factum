# Security Policy

## Reporting a Vulnerability

Factum is an early-stage alpha project. If you discover a security vulnerability:

1. **Do NOT open a public GitHub issue.**
2. Email: **security@factum.dev**

   > **Note:** This address will be active once the domain is registered. Until then, report vulnerabilities via a private GitHub Security Advisory (Security tab → "Report a vulnerability").
3. Include: description, reproduction steps, impact assessment
4. You will receive an acknowledgment within 48 hours.

## Scope

### In Scope
- Parser crashes (stack overflow, panic) on adversarial input
- Permission bypass (accessing nodes the query context should not see)
- Serialization round-trip failures that could cause data corruption
- OOM via pathologically large input (lexer token limit bypass)
- MCP bridge authentication/authorization issues

### Out of Scope
- Performance degradation under load (not a security issue for alpha)
- Issues in dependencies (report upstream)
- Social engineering attacks

## Parser Security

The Factum parser is the trust foundation of the system. The following protections are in place:

| Protection | Mechanism | Constant |
|------------|-----------|----------|
| Stack overflow DoS | Recursive depth limit | `MAX_PARSE_DEPTH = 128` |
| OOM via long input | Token count limit | `MAX_TOKENS = 1,000,000` |
| Adversarial input discovery | cargo-fuzz (parser + serialize + lexer) | CI: 10 min/run |

If you find a way to crash the parser despite these protections, please report it privately.

## Disclosure Timeline

- **Day 0**: Private report received
- **Day 1-2**: Acknowledgment + severity assessment
- **Day 3-14**: Fix development (severity-dependent)
- **Day 15**: Coordinated public disclosure with credit (if desired)

## gitleaks

CI runs [gitleaks](https://github.com/gitleaks/gitleaks) on every push to scan for leaked API keys, tokens, and secrets. If gitleaks finds something, the CI will fail and the commit must be amended to remove the secret.

If you accidentally commit a secret:
1. **Do not just delete it in a new commit** — it's still in git history
2. Use `git filter-repo` or BFG Repo-Cleaner to purge it from history
3. Rotate the compromised credential immediately
