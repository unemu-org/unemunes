//! `unemu-nes-core`: the NES core behind the `unemu-core-api` contract 0.5.
//!
//! A [`NesCore`] describes the core, recognises iNES and NES 2.0 cartridge
//! images (`identify`), checks configs (`validate`) and creates machines;
//! [`NesMachine`] is the machine a front end drives. It follows the
//! contract's conformance list (unemu-core-api `docs/conformance.md`) and
//! the checklist of core-contract.md §13.5 (unemupc) within the proof slice
//! (design note §13): no saved state and no battery memory yet, so the
//! description does not promise them.
//!
//! The crate touches nothing of the host: content arrives through the
//! request's `ContentSource`, and there is no storage to keep yet.

#![forbid(unsafe_code)]

mod config;
mod machine;

pub use crate::config::{NesConfig, MEDIA_TYPE};
pub use crate::machine::NesMachine;

use nes::{Cartridge, CartridgeError, Config};
use unemu_core_api::{
    ConfigDocument, ConfigDocumentBuf, Content, ContentKind, ContentRef, ContractVersion, Core,
    CoreDescription, CreateRequest, Error, ErrorKind, Features, Identified, Machine, Message,
    MessageLevel, OfferedContent, PlatformDescription, ResetKind, Validated,
};

/// The platform id artifacts name; chosen once (conformance item 4).
pub const PLATFORM: &str = "nes";
/// The content kind of iNES and NES 2.0 images.
pub const KIND: &str = "ines";
/// The largest image this core reads: 16 MiB, far above any NROM board.
const MAX_IMAGE: u64 = 16 * 1024 * 1024;

const RESETS: &[ResetKind] = &[ResetKind::Soft, ResetKind::Hard];
pub(crate) const FEATURES: Features = Features::NONE.with_resets(RESETS).with_frame_stops(true);
const CONTENT: &[ContentKind] = &[ContentKind::new(KIND, "NES cartridge images", &["nes"])];
const CONFIG_TYPES: &[&str] = &[MEDIA_TYPE];
const PLATFORMS: &[PlatformDescription] =
    &[
        PlatformDescription::new(PLATFORM, "Nintendo Entertainment System (NTSC)")
            .with_config_types(CONFIG_TYPES)
            .with_features(FEATURES)
            .with_content(CONTENT),
    ];
const DESCRIPTION: CoreDescription = CoreDescription::new(
    ContractVersion::new(0, 5),
    "unemunes",
    "unemunes",
    env!("CARGO_PKG_VERSION"),
)
.with_platforms(PLATFORMS);

/// The NES core.
#[derive(Clone, Copy, Debug, Default)]
pub struct NesCore;

impl Core for NesCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn identify(&self, content: &OfferedContent<'_>) -> Result<Option<Identified>, Error> {
        if content.size < 4 {
            return Ok(None);
        }
        let mut header = [0u8; 4];
        content.bytes.read_at(0, &mut header)?;
        if header != *b"NES\x1A" {
            return Ok(None);
        }
        if content.size < 16 {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "the image is shorter than its 16-byte header",
            ));
        }
        if content.size > MAX_IMAGE {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "the image is larger than any cartridge this core runs",
            ));
        }
        let mut bytes = vec![0u8; content.size as usize];
        content.bytes.read_at(0, &mut bytes)?;
        let cartridge = cartridge(&bytes)?;
        let mut notes = Vec::new();
        if cartridge.region != nes::Region::Ntsc {
            notes.push(Message::new(
                MessageLevel::Warning,
                format!(
                    "the image asks for {:?} timing; this core runs it on an NTSC console",
                    cartridge.region
                ),
            ));
        }
        let config = NesConfig::for_cartridge(content.sha256, content.size);
        Ok(Some(
            Identified::new(
                PLATFORM,
                KIND,
                ConfigDocumentBuf::new(MEDIA_TYPE, config.to_document()),
            )
            .with_notes(notes),
        ))
    }

    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        check_platform(platform)?;
        let config = NesConfig::parse(config.media_type, config.bytes)?;
        check_size(config.cartridge_size)?;
        Ok(Validated::new(vec![ContentRef::new(
            config.cartridge_sha256,
            config.cartridge_size,
            "cartridge",
        )]))
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        check_platform(request.platform)?;
        if let Some((key, _)) = request.options.first() {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("option {key} is not one this core takes"),
            ));
        }
        let config = NesConfig::parse(request.config.media_type, request.config.bytes)?;
        check_size(config.cartridge_size)?;
        let bytes = read_all(
            request
                .content
                .open(&config.cartridge_sha256, config.cartridge_size)?,
            config.cartridge_size,
        )?;
        let cartridge = cartridge(&bytes)?;
        let power_on = Config {
            ram_fill: config.ram_fill,
            dma_get_on_odd: config.dma_get_on_odd,
        };
        Ok(Box::new(NesMachine::new(cartridge, power_on, config)?))
    }
}

fn check_platform(platform: &str) -> Result<(), Error> {
    if platform == PLATFORM {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::Unsupported,
            format!("platform {platform} is not one this core provides"),
        ))
    }
}

/// Parse an image: one that is not a cartridge, is damaged, or needs a board
/// this core does not model is `Unsupported`, with the reason. The bytes are
/// the verified ones, so none of these is `Content`.
fn cartridge(bytes: &[u8]) -> Result<Cartridge, Error> {
    let cartridge = Cartridge::from_ines(bytes)
        .map_err(|error: CartridgeError| Error::new(ErrorKind::Unsupported, error.to_string()))?;
    nes::mapper::board(&cartridge)
        .map_err(|error| Error::new(ErrorKind::Unsupported, error.to_string()))?;
    Ok(cartridge)
}

fn check_size(size: u64) -> Result<(), Error> {
    if size > MAX_IMAGE {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "the image is larger than any cartridge this core runs",
        ));
    }
    Ok(())
}

fn read_all(content: Content, size: u64) -> Result<Vec<u8>, Error> {
    match content {
        Content::Bytes(bytes) => Ok(bytes),
        Content::Reader(reader) => {
            let mut bytes = vec![0u8; size as usize];
            reader.read_at(0, &mut bytes)?;
            Ok(bytes)
        }
        _ => Err(Error::new(
            ErrorKind::Unsupported,
            "a form of content this core does not read",
        )),
    }
}
