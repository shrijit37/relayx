/**
 * Minimal secret resolver.
 *
 * Phase 6 keeps it small: a lane stores a `credential_ref` pointing at an
 * environment variable (`provider: "env"`). Resolution yields the raw secret
 * at publish time; the secret itself is never stored in the DB, workflow JSON,
 * API responses, or logs. `provider: "vault"` is a documented future seam —
 * it resolves to null (auth omitted) rather than failing the publish.
 */

export type CredentialRef = {
  ref: string;
  provider: "env" | "vault";
};

/** Resolve a credential reference to an Authorization header value.
 *  For vault references (Phase 7), returns null — traffic will flow
 *  without an Authorization header. If the upstream requires one,
 *  expect a 401. Log a warning so the silent-drop isn't invisible. */
export function resolveCredential(ref: CredentialRef | null): string | null {
  if (!ref) return null;
  if (ref.provider === "vault") {
    console.warn(
      `[secrets] vault credential "${ref.ref}" resolved to no Authorization header — upstream 401s are expected until the vault backend is wired`,
    );
    return null;
  }
  const value = Bun.env[ref.ref];
  if (!value) {
    console.warn(
      `[secrets] env credential "${ref.ref}" not found in environment — lane will have no Authorization header`,
    );
  }
  return value ? `Bearer ${value}` : null;
}