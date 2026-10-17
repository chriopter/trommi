/** Tinfoil's attestation verifier (core/tinfoil-verifier.mjs): checks an attestation bundle offline against the code of `configRepo`. */
export class Verifier {
  constructor(options: { configRepo: string; serverURL?: string })
  /** The enclave's HPKE key (hex) and TLS key fingerprint as its attestation binds them; throws when it does not check out. */
  verifyBundle(bundle: unknown): Promise<{ hpkePublicKey?: string; tlsPublicKeyFingerprint?: string; measurement: { type: string; registers: string[] } }>
}
