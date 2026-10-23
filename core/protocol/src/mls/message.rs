//! MLS application messages (section 7): the four things that travel as a `PrivateMessage` and are never
//! stored content. This module is their encoding; who may send which in which group is the device's to check.

use crate::codec::{Decode, Encode, Reader, Writer};
use crate::crypto::Secret;
use crate::error::Error;
use crate::ids::{BoardId, DeviceId, GroupId, TurnId};
use crate::mls::profile::RECOVERY_KEY_LEN;

/// The most bytes of an application message, as plaintext and as it travels (7.0).
pub const MAX_MESSAGE_LEN: usize = 48 * 1024;
/// The most keys one key handover carries, so that it stays below [`MAX_MESSAGE_LEN`].
pub const MAX_HANDOVER_KEYS: usize = 400;
/// The version every message names.
pub const MESSAGE_VERSION: u8 = 2;

/// What MLS adds to a message's plaintext inside the encrypted content (the length, the signature, the AEAD
/// tag), counted generously, so that the padded content holds it (7.0).
const CONTENT_OVERHEAD: usize = 128;
/// The largest bucket: what is left of [`MAX_MESSAGE_LEN`] for the encrypted content beside the clear framing.
pub const TOP_BUCKET: usize = MAX_MESSAGE_LEN - 256;
/// The largest plaintext a message can have padded.
pub const MAX_PLAINTEXT_LEN: usize = TOP_BUCKET - CONTENT_OVERHEAD;

/// 7.0: the size a message's encrypted MLS content is padded to: the first of 512, 1024 … 32768 bytes it fits
/// in, and above that [`TOP_BUCKET`]. `None` for a plaintext above [`MAX_PLAINTEXT_LEN`].
pub fn bucket(plaintext_len: usize) -> Option<usize> {
    let need = plaintext_len.checked_add(CONTENT_OVERHEAD)?;
    let mut size = 512;
    while size < need && size < 32768 {
        size *= 2;
    }
    if need <= size {
        Some(size)
    } else if need <= TOP_BUCKET {
        Some(TOP_BUCKET)
    } else {
        None
    }
}

const TYPE_KEY_HANDOVER: u8 = 1;
const TYPE_STROKE_PIECE: u8 = 2;
const TYPE_WORK_TRAIL: u8 = 3;
const TYPE_RECOVERY_AUTH: u8 = 4;
const TYPE_ANSWER_PREVIEW: u8 = 5;

/// The content key of one group and epoch, as a handover carries it.
#[derive(Debug, PartialEq, Eq)]
pub struct EpochKey {
    /// The group.
    pub group: GroupId,
    /// The epoch.
    pub epoch: u64,
    /// `content_key(group, epoch)`.
    pub content_key: Secret<32>,
}

impl Encode for EpochKey {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.value(&self.group)?;
        writer.u64(self.epoch);
        writer.fixed(self.content_key.expose());
        Ok(())
    }
}

impl Decode for EpochKey {
    fn read(reader: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Self {
            group: reader.value()?,
            epoch: reader.u64()?,
            content_key: Secret::new(reader.fixed()?),
        })
    }
}

/// One application message.
#[derive(Debug, PartialEq, Eq)]
pub enum TrommiMessage {
    /// Old content keys for a device that was just added or took a session over (7.1).
    KeyHandover {
        /// The device it is for.
        recipient: DeviceId,
        /// The keys.
        keys: Vec<EpochKey>,
        /// Whether this is the final message of the handover.
        last: bool,
    },
    /// The points of a stroke still being drawn (7.2).
    StrokePiece {
        /// The board.
        board: BoardId,
        /// The JSON piece entry.
        piece: Vec<u8>,
    },
    /// One step of an agent's running turn (7.3).
    WorkTrail {
        /// The turn.
        turn: TurnId,
        /// The step's number within the turn, from 1.
        number: u32,
        /// The sender's clock, in milliseconds.
        time: u64,
        /// The JSON step.
        step: Vec<u8>,
    },
    /// The answer of an agent's running turn as it stands, relayed and never stored (7.5).
    AnswerPreview {
        /// The turn.
        turn: TurnId,
        /// The preview's number within the turn, from 1: a newer one replaces an older one.
        number: u32,
        /// The JSON preview.
        preview: Vec<u8>,
    },
    /// The `recovery_mac` of a recovery key, for a human device that does not hold it (7.4).
    RecoveryAuth {
        /// The device it is for, or zeros for all.
        recipient: DeviceId,
        /// The recovery key it belongs to.
        recovery_hpke_key: [u8; RECOVERY_KEY_LEN],
        /// The key that authenticates sealed rows.
        recovery_mac: Secret<32>,
    },
}

impl Encode for TrommiMessage {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.u8(MESSAGE_VERSION);
        match self {
            Self::KeyHandover {
                recipient,
                keys,
                last,
            } => {
                writer.u8(TYPE_KEY_HANDOVER);
                writer.value(recipient)?;
                writer.vector(keys)?;
                writer.u8(u8::from(*last));
            }
            Self::StrokePiece { board, piece } => {
                writer.u8(TYPE_STROKE_PIECE);
                writer.value(board)?;
                writer.opaque(piece)?;
            }
            Self::WorkTrail {
                turn,
                number,
                time,
                step,
            } => {
                writer.u8(TYPE_WORK_TRAIL);
                writer.value(turn)?;
                writer.u32(*number);
                writer.u64(*time);
                writer.opaque(step)?;
            }
            Self::AnswerPreview {
                turn,
                number,
                preview,
            } => {
                writer.u8(TYPE_ANSWER_PREVIEW);
                writer.value(turn)?;
                writer.u32(*number);
                writer.opaque(preview)?;
            }
            Self::RecoveryAuth {
                recipient,
                recovery_hpke_key,
                recovery_mac,
            } => {
                writer.u8(TYPE_RECOVERY_AUTH);
                writer.value(recipient)?;
                writer.opaque(recovery_hpke_key)?;
                writer.fixed(recovery_mac.expose());
            }
        }
        Ok(())
    }
}

impl Decode for TrommiMessage {
    fn read(reader: &mut Reader<'_>) -> Result<Self, Error> {
        match reader.u8()? {
            MESSAGE_VERSION => {}
            version if version > MESSAGE_VERSION => return Err(Error::NewerVersion),
            _ => return Err(Error::BadFormat),
        }
        match reader.u8()? {
            TYPE_KEY_HANDOVER => Ok(Self::KeyHandover {
                recipient: reader.value()?,
                keys: reader.vector()?,
                last: match reader.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(Error::BadFormat),
                },
            }),
            TYPE_STROKE_PIECE => Ok(Self::StrokePiece {
                board: reader.value()?,
                piece: reader.opaque()?.to_vec(),
            }),
            TYPE_WORK_TRAIL => Ok(Self::WorkTrail {
                turn: reader.value()?,
                number: reader.u32()?,
                time: reader.u64()?,
                step: reader.opaque()?.to_vec(),
            }),
            TYPE_ANSWER_PREVIEW => Ok(Self::AnswerPreview {
                turn: reader.value()?,
                number: reader.u32()?,
                preview: reader.opaque()?.to_vec(),
            }),
            TYPE_RECOVERY_AUTH => Ok(Self::RecoveryAuth {
                recipient: reader.value()?,
                recovery_hpke_key: reader.opaque()?.try_into().map_err(|_| Error::BadFormat)?,
                recovery_mac: Secret::new(reader.fixed()?),
            }),
            _ => Err(Error::BadFormat),
        }
    }
}
