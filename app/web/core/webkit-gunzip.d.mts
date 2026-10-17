/** Wraps WebKit's DecompressionStream so a Response body can be piped into it (before Tinfoil's verifier runs). */
export function fixDecompression(): void
