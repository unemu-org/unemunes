//! The config document of a NES machine: the core's own format (contract
//! 0.2, `ConfigDocumentBuf`; core-contract.md §13.2 (unemupc)).
//!
//! One `key = value` pair per line; blank lines and lines starting with `#`
//! are ignored. The front end treats the bytes as opaque. Keys:
//!
//! | Key | Value |
//! | --- | --- |
//! | `cartridge.sha256` | 64 hex digits, required |
//! | `cartridge.size` | bytes, required |
//! | `region` | `ntsc`, the only region implemented (design note §14 Q8) |
//! | `ram-fill` | the byte internal RAM holds at power-on, `0x00`–`0xFF` or decimal; default 0 |
//! | `dma-get` | `even` or `odd`: the CPU cycles DMA gets on; default `even` |
//!
//! The power-on values are design note §14 Q7's.

use unemu_core_api::{Diagnostic, Error, ErrorKind};

/// The media type of the document, versioned.
pub const MEDIA_TYPE: &str = "application/vnd.unemu.unemunes.config.v1";

/// A parsed config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NesConfig {
    pub cartridge_sha256: [u8; 32],
    pub cartridge_size: u64,
    pub ram_fill: u8,
    pub dma_get_on_odd: bool,
}

impl NesConfig {
    /// A config with the platform's defaults for a cartridge.
    pub fn for_cartridge(sha256: [u8; 32], size: u64) -> NesConfig {
        NesConfig {
            cartridge_sha256: sha256,
            cartridge_size: size,
            ram_fill: 0,
            dma_get_on_odd: false,
        }
    }

    /// The document's bytes. The same config always writes the same bytes.
    pub fn to_document(&self) -> Vec<u8> {
        let mut hex = String::with_capacity(64);
        for byte in self.cartridge_sha256 {
            hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            hex.push(char::from_digit(u32::from(byte & 15), 16).unwrap_or('0'));
        }
        format!(
            "# unemunes machine config v1\ncartridge.sha256 = {hex}\ncartridge.size = {}\nregion = ntsc\nram-fill = {}\ndma-get = {}\n",
            self.cartridge_size,
            self.ram_fill,
            if self.dma_get_on_odd { "odd" } else { "even" }
        )
        .into_bytes()
    }

    /// Parse a document; errors are `ErrorKind::Config` with a diagnostic per
    /// problem.
    pub fn parse(media_type: &str, bytes: &[u8]) -> Result<NesConfig, Error> {
        if media_type != MEDIA_TYPE {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("config type {media_type} is not one this core reads"),
            ));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::new(ErrorKind::Config, "the config is not UTF-8 text"))?;
        let mut sha = None;
        let mut size = None;
        let mut ram_fill = 0u8;
        let mut dma_get_on_odd = false;
        let mut problems = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                problems.push(Diagnostic::new(
                    format!("line {}", number + 1),
                    "expected key = value",
                ));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let mut bad = |message: &str| problems.push(Diagnostic::new(key, message));
            match key {
                "cartridge.sha256" => match parse_sha256(value) {
                    Some(digest) => sha = Some(digest),
                    None => bad("expected 64 hex digits"),
                },
                "cartridge.size" => match value.parse::<u64>() {
                    Ok(n) => size = Some(n),
                    Err(_) => bad("expected a size in bytes"),
                },
                "region" if value == "ntsc" => {}
                "region" => bad("only ntsc is implemented"),
                "ram-fill" => match parse_byte(value) {
                    Some(b) => ram_fill = b,
                    None => bad("expected a byte, 0x00 to 0xFF"),
                },
                "dma-get" => match value {
                    "even" => dma_get_on_odd = false,
                    "odd" => dma_get_on_odd = true,
                    _ => bad("expected even or odd"),
                },
                _ => bad("unknown key"),
            }
        }
        if sha.is_none() {
            problems.push(Diagnostic::new("cartridge.sha256", "missing"));
        }
        if size.is_none() {
            problems.push(Diagnostic::new("cartridge.size", "missing"));
        }
        match (sha, size) {
            (Some(cartridge_sha256), Some(cartridge_size)) if problems.is_empty() => {
                Ok(NesConfig {
                    cartridge_sha256,
                    cartridge_size,
                    ram_fill,
                    dma_get_on_odd,
                })
            }
            _ => {
                Err(Error::new(ErrorKind::Config, "the config is not valid")
                    .with_diagnostics(problems))
            }
        }
    }
}

fn parse_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn parse_byte(value: &str) -> Option<u8> {
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_config_reads_back() {
        let mut config = NesConfig::for_cartridge([0xAB; 32], 40976);
        config.ram_fill = 0xFF;
        config.dma_get_on_odd = true;
        let bytes = config.to_document();
        assert_eq!(NesConfig::parse(MEDIA_TYPE, &bytes).unwrap(), config);
    }

    #[test]
    fn problems_come_back_as_diagnostics() {
        let error = NesConfig::parse(MEDIA_TYPE, b"region = pal\nbogus = 1\n").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Config);
        let paths: Vec<&str> = error.diagnostics.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(
            paths,
            ["region", "bogus", "cartridge.sha256", "cartridge.size"]
        );
    }

    #[test]
    fn another_media_type_is_unsupported() {
        let error = NesConfig::parse("text/plain", b"").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
    }
}
