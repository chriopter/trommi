//! The constants of the envelope (spec/v1.md section 9): its version, kinds, the fields of its header, its forms
//! and the labels of its hash and of an object's id.

/// The version of the header and of the body.
pub const VERSION: u8 = 2;

/// The kinds of an envelope (9.0.4); 8 to 255 are reserved.
pub const KIND_ITEM: u8 = 1;
pub const KIND_VERSION: u8 = 2;
pub const KIND_ANSWER: u8 = 3;
pub const KIND_REQUEST: u8 = 4;
pub const KIND_VERDICT: u8 = 5;
pub const KIND_REGISTER: u8 = 6;
pub const KIND_TAKE_BACK: u8 = 7;
/// The first of the kinds 8 to 255, which no writer may use.
pub const FIRST_RESERVED_KIND: u8 = 8;

pub const TYPE_CARD: u8 = 1;
pub const TYPE_NOTE: u8 = 2;
pub const TYPE_REQUEST: u8 = 3;
pub const TYPE_ARTIFACT: u8 = 4;

pub const STATE_OPEN: u8 = 1;
pub const STATE_ANSWERED: u8 = 2;
pub const STATE_CLOSED: u8 = 3;

pub const TIMELINE_CHAT: u8 = 1;
pub const TIMELINE_BOARD: u8 = 2;
pub const SCOPE_CARD: u8 = 1;
pub const SCOPE_SESSION: u8 = 2;
pub const SCOPE_DESK: u8 = 3;

/// Flag bit 0: the sender asks for a push.
pub const FLAG_PUSH: u8 = 1;

/// The envelope's first byte: with its sealed body, or pruned to the body's hash.
pub const FORM_FULL: u8 = 1;
pub const FORM_PRUNED: u8 = 2;

/// How many files a header names at most.
pub const MAX_FILE_IDS: usize = 255;
/// The length of an Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// `envelope_hash = RefHash(LABEL_HASH, Header ‖ nonce ‖ SHA-256(ciphertext))`.
pub const LABEL_HASH: &str = "Trommi Envelope";
/// `object_id` of a first version: the first 16 bytes of `RefHash(LABEL_OBJECT, group_id ‖ sender ‖ seq)`.
pub const LABEL_OBJECT: &str = "Trommi Object";
