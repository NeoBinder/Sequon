//! LZMA decoding for history blocks, with allocation and expansion caps.
//!
//! The codec is lzma-rs 0.3.0 (MIT, pure Rust), pinned and documented in
//! docs/design/snapgene-dna.md section 2. The LZMA-alone framing (13-byte
//! header: properties, dictionary size, unpacked size) is the documented
//! hypothesis F-20; the pre-check treats the header as that framing and falls
//! back to the writer caps if the payload is a raw stream.

use std::io::{self, Write};

use lzma_rs::decompress::Options;

use crate::error::ImportError;
use crate::limits::Limits;
use crate::report::SourcePosition;

/// Failing sink that stops LZMA decoding once a configured cap is crossed.
struct CappedWriter {
    output: Vec<u8>,
    max_bytes: u64,
    max_ratio: u64,
    input_bytes: u64,
    hit: Option<&'static str>,
}

impl CappedWriter {
    fn new(max_bytes: u64, max_ratio: u64, input_bytes: u64) -> Self {
        Self {
            output: Vec::new(),
            max_bytes,
            max_ratio,
            input_bytes,
            hit: None,
        }
    }
}

impl Write for CappedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let new_len = self.output.len() as u64 + buffer.len() as u64;
        if new_len > self.max_bytes {
            self.hit = Some("max_lzma_output_bytes");
            return Err(io::Error::other(
                "decompressed output exceeds the configured limit",
            ));
        }
        let ratio_limit = self
            .input_bytes
            .max(1)
            .saturating_mul(self.max_ratio.max(1));
        if new_len > ratio_limit {
            self.hit = Some("max_lzma_ratio");
            return Err(io::Error::other(
                "decompressed output exceeds the configured ratio",
            ));
        }
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Decompresses one history payload, enforcing the LZMA limits.
pub(crate) fn decompress(
    payload: &[u8],
    limits: &Limits,
    position: SourcePosition,
) -> Result<Vec<u8>, ImportError> {
    if payload.len() < 13 {
        return Err(ImportError::Malformed {
            code: "DNA_LZMA_MALFORMED",
            message: "the LZMA stream is shorter than its header".to_string(),
            position: Some(position),
        });
    }
    let properties = payload[0];
    if properties >= 225 {
        return Err(ImportError::Malformed {
            code: "DNA_LZMA_MALFORMED",
            message: "the LZMA properties byte is outside the valid range".to_string(),
            position: Some(position),
        });
    }
    let dictionary = u64::from(u32::from_le_bytes([
        payload[1], payload[2], payload[3], payload[4],
    ]))
    .max(0x1000);
    if dictionary > limits.max_lzma_dict_bytes {
        return Err(decompression_limit(
            "max_lzma_dict_bytes",
            limits.max_lzma_dict_bytes,
            dictionary,
            position,
        ));
    }
    let declared = u64::from_le_bytes([
        payload[5],
        payload[6],
        payload[7],
        payload[8],
        payload[9],
        payload[10],
        payload[11],
        payload[12],
    ]);
    if declared != u64::MAX && declared > limits.max_lzma_output_bytes {
        return Err(decompression_limit(
            "max_lzma_output_bytes",
            limits.max_lzma_output_bytes,
            declared,
            position,
        ));
    }
    let memlimit = usize::try_from(limits.max_lzma_dict_bytes).unwrap_or(usize::MAX);
    let options = Options {
        memlimit: Some(memlimit),
        ..Options::default()
    };
    let mut writer = CappedWriter::new(
        limits.max_lzma_output_bytes,
        limits.max_lzma_ratio,
        payload.len() as u64,
    );
    match lzma_rs::lzma_decompress_with_options(&mut &payload[..], &mut writer, &options) {
        Ok(()) => Ok(writer.output),
        Err(error) => match writer.hit {
            Some(limit) => Err(decompression_limit(
                limit,
                if limit == "max_lzma_output_bytes" {
                    limits.max_lzma_output_bytes
                } else {
                    limits.max_lzma_ratio
                },
                writer.output.len() as u64,
                position,
            )),
            None => Err(ImportError::Malformed {
                code: "DNA_LZMA_MALFORMED",
                message: format!("the LZMA stream could not be decoded ({error})"),
                position: Some(position),
            }),
        },
    }
}

fn decompression_limit(
    limit: &'static str,
    limit_value: u64,
    actual: u64,
    position: SourcePosition,
) -> ImportError {
    ImportError::Malformed {
        code: "DNA_DECOMPRESSION_LIMIT",
        message: format!("{limit} exceeded: configured {limit_value}, observed {actual}"),
        position: Some(position),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::report::SourcePosition;

    fn position() -> SourcePosition {
        SourcePosition::Snapgene {
            offset: 100,
            packet_type: Some(7),
            packet_index: Some(3),
        }
    }

    fn compressed(data: &[u8]) -> Vec<u8> {
        let options = lzma_rs::compress::Options {
            unpacked_size: lzma_rs::compress::UnpackedSize::WriteToHeader(Some(data.len() as u64)),
        };
        let mut output = Vec::new();
        lzma_rs::lzma_compress_with_options(&mut &data[..], &mut output, &options)
            .expect("in-memory compression");
        output
    }

    #[test]
    fn decodes_normal_history_payload() {
        let limits = Limits::default();
        let data = b"<History><Step/></History>";
        let decoded = decompress(&compressed(data), &limits, position()).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn rejects_output_over_the_ratio_cap() {
        let limits = Limits {
            max_lzma_ratio: 2,
            ..Limits::default()
        };
        let data = vec![b'A'; 4096];
        let error = decompress(&compressed(&data), &limits, position()).expect_err("ratio");
        assert_eq!(error.code(), "DNA_DECOMPRESSION_LIMIT");
    }

    #[test]
    fn rejects_output_over_the_absolute_cap() {
        let limits = Limits {
            max_lzma_output_bytes: 128,
            ..Limits::default()
        };
        let data = vec![b'A'; 4096];
        let error = decompress(&compressed(&data), &limits, position()).expect_err("size");
        assert_eq!(error.code(), "DNA_DECOMPRESSION_LIMIT");
    }

    #[test]
    fn rejects_dictionary_over_the_cap() {
        let limits = Limits {
            max_lzma_dict_bytes: 1024,
            ..Limits::default()
        };
        let mut payload = compressed(b"hello");
        payload[1..5].copy_from_slice(&(1_u32 << 20).to_le_bytes());
        let error = decompress(&payload, &limits, position()).expect_err("dict");
        assert_eq!(error.code(), "DNA_DECOMPRESSION_LIMIT");
    }

    #[test]
    fn rejects_truncated_stream() {
        let limits = Limits::default();
        let payload = compressed(b"some history xml payload");
        let error =
            decompress(&payload[..payload.len() / 2], &limits, position()).expect_err("truncated");
        assert_eq!(error.code(), "DNA_LZMA_MALFORMED");
    }
}
