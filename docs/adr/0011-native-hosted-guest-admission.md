# Native hosted guest admission

Status: implemented for #733 stage 5; complete missing-asset and two-network acceptance remain stage 6.

## Context

Atlas 3.1.i145 through 3.1.i145-6 require individually approved, world-scoped guest access and durable characters. A public invitation locates a world; it does not grant gameplay or permit a caller to claim another character. The development/LAN character service deliberately accepts caller-selected IDs and therefore cannot be exposed unchanged to public guests.

## Decision

Compose the optional game-server WebRTC attachment with the existing prepared live host, character storage, parties and recovery. The prepared host remains the sole tick/lifecycle owner; WebRTC attaches to its same match and publication source. The native owner's WebTransport listener binds IPv4 loopback. Its character claim carries a per-run private local-control capability's HMAC, bound to the server player ID, connection epoch and complete claim. Unauthenticated arbitrary-ID claims are rejected on managed worlds. Dedicated development/LAN serving retains its existing trust mode.

A guest's local roster slot owns an Ed25519 signing key in an OS-private `guest-identities/` directory (0700 directory / 0600 key on Unix). This is a game-owned returning-guest identity, distinct from ephemeral setup capabilities, tickets, TURN credentials and rotating reconnect tokens. No private key is transmitted. The 96-byte admission proof is the 32-byte public key and 64-byte signature over `mmorpg-guest-v1\0<lobbyId>\0<participantId>\0<matchId>`. The setup service authenticates the participant independently. Binding that participant and lobby prevents forwarding a signed proof to another participant/world; setup capabilities and all transport secrets remain memory-only.

The game verifies this proof before game-server issues its admission ticket. Unknown keys wait for the local owner's Approve/Deny action; a short public key code is shown to both parties. The host persists only public keys and their world-owned, randomly assigned `CharacterId` entitlements. A valid returning key resumes the same character without a new owner approval. Its proof never lets the client select a durable character ID. Character claims translate the authenticated participant's entitlement into the existing exclusive fenced character lease and trusted admission input. The existing local roster class must match the saved character; mismatches fail closed.

Remove denies the entitlement immediately; game-server revalidation fences commands/control/publications and closes the connection. Records and progression remain intact. Reapproval retains the assigned character. Existing disconnected-session grace and lease release still apply; a returning client drains projections while waiting for the prior holder's normal release, rather than taking a second lease. Neither an invitation nor a reconnect token replaces the durable guest key, and there is no automatic cross-world character transfer or account recovery.

Registration uses the existing lobby API and its bounded lifetime/renewal. The ready local host registers, then exposes the canonical public `https://moenarch.com/native.html?lobbyId=…&matchId=…` URL through private local status. Both native paste and `--join-invite` use one strict parser with a fixed setup origin; no URL-supplied endpoint, executable, credential or additional parameter is accepted. A setup failure clears sharing status; local owner gameplay and saves remain independent. Stop World stops the authority and invalidates actual joins. Public lobby metadata can outlive the host until setup expiry, so neither the website nor native code treats metadata existence as proof of readiness.

## Validation and limits

`scripts/smoke-hosted-native.py` starts a real loopback setup service, coturn, zone-host process and two headless native session processes. It checks authoritative movement, owner approval, refusal to use a supplied owner `CharacterId`, reconnect/epoch rotation, continued guest play after the owner client exits, removal, saved-character return after host restart, stale invitation rejection and clearing invitation status after signaling fails. Its selected local ICE path is reported as observed; localhost is not NAT traversal evidence. This check does not drive a native window, fetch missing shareable art, or complete #733's stage 6 acceptance.
