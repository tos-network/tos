# ProtonMail-Style End-to-End Encryption and TOS Address Messaging

This note explains how ProtonMail-style end-to-end encryption can align with TOS address-based encrypted messaging, where only the recipient's public key can decrypt.

## Quick answer

Yes, the mechanisms are compatible at a design level. ProtonMail's model (public-key encryption + digital signatures + local decryption) maps well onto a blockchain messaging flow where:

- the sender encrypts to the recipient's public key,
- the ciphertext is stored or transported by the network, and
- only the recipient can decrypt locally with their private key.

The key challenge is not the cryptography itself, but key discovery, key authenticity, metadata privacy, and how much data you put on-chain.

## How ProtonMail works (simplified)

ProtonMail's end-to-end security model is based on a few core ideas:

1. **Public-key encryption for confidentiality.**
   - A sender encrypts a message to the recipient's public key.
   - Only the matching private key can decrypt.

2. **Digital signatures for authenticity.**
   - The sender signs the message (or a digest of it) with their private key.
   - Recipients verify using the sender's public key.

3. **Client-side decryption.**
   - Servers store ciphertext and metadata.
   - Decryption happens on the client, after key checks.

In practice, ProtonMail uses OpenPGP-compatible constructions and hybrid encryption (a symmetric content key protected by public keys).

## Mapping this to TOS address messaging

Your current TOS capability — "send encrypted messages to an address, and only the holder of that public key can decrypt" — already mirrors the most important part of ProtonMail's model.

At a high level, the flow can be:

1. **Key binding**
   - Each address is associated with a long-term encryption public key (and, ideally, a signing public key).
   - The binding between address and key must be authentic and discoverable.

2. **Encryption**
   - The sender generates a random symmetric key.
   - The message is encrypted with an authenticated symmetric cipher.
   - The symmetric key is encrypted to the recipient's public key.

3. **Authentication**
   - The sender signs the ciphertext envelope (or its hash).
   - The signature is verifiable against the sender's address-bound public key.

4. **Transport / storage**
   - The encrypted payload can be:
     - stored directly on-chain (simple but expensive and public), or
     - stored off-chain with only commitments, hashes, pointers, or access tickets on-chain (usually better).

5. **Decryption**
   - The recipient fetches the ciphertext and decrypts locally with their private key.

## What you should add beyond "encrypt to public key"

To get closer to ProtonMail-grade guarantees, most systems need additional pieces around the core encryption step:

### 1) On-chain key authenticity and rotation

You will likely need a canonical, on-chain way to answer:

- "What is the correct encryption key for this address right now?"
- "Has the key been rotated or revoked?"

Common patterns:

- A signed on-chain key registry transaction.
- Versioned keys with explicit rotation and revocation.
- Separation of concerns between:
  - an address identity key, and
  - one or more encryption subkeys.

### 2) Hybrid encryption with explicit envelopes

Use a hybrid scheme with a stable envelope format:

- `content_key = random()`
- `ciphertext = AEAD_Encrypt(content_key, plaintext, associated_data)`
- `sealed_key = AsymmetricEncrypt(recipient_pubkey, content_key)`
- `signature = Sign(sender_signing_key, hash(envelope))`

The associated data should include relevant routing and integrity fields (for example: sender address, recipient address, chain id, key version, and a timestamp or nonce).

### 3) Metadata privacy expectations

Even when the message body is encrypted, on-chain messaging can leak:

- who talks to whom,
- when they communicate,
- how much they communicate, and
- message size patterns.

This is not a reason to avoid the design, but it should be stated clearly and mitigated where possible (for example, batching, padding, and off-chain transport).

### 4) On-chain vs off-chain data placement

Putting full ciphertext on-chain is straightforward but often suboptimal:

- it is permanent,
- it is public,
- it is expensive,
- and it can create compliance or data lifecycle concerns.

A common architecture is:

- store the ciphertext off-chain (for example: object storage or a content-addressed store),
- store only a hash/commitment plus a pointer or retrieval hint on-chain.

## Practical integration directions

If you want to explicitly align with ProtonMail's design philosophy while staying blockchain-native, a pragmatic path is:

1. **Define a message envelope format** (versioned, signed, hybrid-encrypted).
2. **Define an on-chain key registry** for encryption keys, with rotation/revocation.
3. **Store minimal data on-chain** (hashes, pointers, receipts), keep ciphertext off-chain.
4. **Make signature verification first-class** so recipients can prove authorship.
5. **Document threat models explicitly** (confidentiality vs metadata privacy vs availability).

## Bottom line

ProtonMail's mechanism and TOS address-based encrypted messaging are conceptually compatible. The most important work is around key management, authenticity, and data placement — not the encryption primitive itself.
