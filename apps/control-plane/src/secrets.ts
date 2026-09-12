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

/** Resolve a credential reference to an Authorization header value. */
export function resolveCredential(ref: CredentialRef | null): string | null {
  if (!ref) return null;
  if (ref.provider === "vault") return null; // Phase 7 secret backend
  const value = Bun.env[ref.ref];
  return value ? `Bearer ${value}` : null;
}