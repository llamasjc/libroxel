// The cryptography the website needs to sign in to a PlayLibre Hub, the same
// way the app does: Argon2id, ChaCha20-Poly1305, Ed25519 and SHA-256.
import * as ed from '@noble/ed25519';
import { sha512, sha256 } from '@noble/hashes/sha2';
import { chacha20poly1305 } from '@noble/ciphers/chacha';
import { argon2id } from 'hash-wasm';

ed.etc.sha512Sync = (...m) => sha512(ed.etc.concatBytes(...m));

export const publicKey = (secret) => ed.getPublicKey(secret);
export const sign = (secret, message) => ed.sign(message, secret);
export const sha256Bytes = (bytes) => sha256(bytes);
export const seal = (key, nonce, plain, aad) => chacha20poly1305(key, nonce, aad).encrypt(plain);
export const open = (key, nonce, sealed, aad) => chacha20poly1305(key, nonce, aad).decrypt(sealed);
export const stretch = (password, salt, memoryKib, passes) =>
  argon2id({ password, salt, parallelism: 1, iterations: passes, memorySize: memoryKib, hashLength: 64, outputType: 'binary' });
