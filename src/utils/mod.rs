pub mod binary_chopper;
pub mod runner;

use scale_value::{Composite, Value, ValueDef};

/// Our stdout lock is io::Write but we need fmt::Write for scale_value writing.
/// Ideally we'd change scale_value, but io::Write is std-only among other things, 
/// so scale-value uses fmt::Write to be no-std.
pub struct ToFmtWrite<W>(pub W);
impl <W: std::io::Write> std::fmt::Write for ToFmtWrite<W> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes()).map(|_| ()).map_err(|_| std::fmt::Error)
    }
}
impl <W: std::io::Write> std::io::Write for ToFmtWrite<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Write out a pretty Value using `std::io::Write`.
pub fn write_value<W: std::io::Write, T: std::fmt::Display>(w: W, value: &Value<T>) -> core::fmt::Result {
    write_value_fmt(ToFmtWrite(w), value)
}

/// Write out a pretty Value using `std::fmt::Write`.
pub fn write_value_fmt<W: std::fmt::Write, T: std::fmt::Display>(w: W, value: &Value<T>) -> core::fmt::Result {
    scale_value::stringify::to_writer_custom()
        .pretty()
        .format_context(|type_id, w: &mut W| write!(w, "{type_id}"))
        .add_custom_formatter(|v, w: &mut W| scale_value::stringify::custom_formatters::format_hex(v,w))
        .add_custom_formatter(|v, w: &mut W| {
            // don't space unnamed composites over multiple lines if lots of primitive values.
            if let ValueDef::Composite(Composite::Unnamed(vals)) = &v.value {
                let are_primitive = vals.iter().all(|val| matches!(val.value, ValueDef::Primitive(_)));
                if are_primitive {
                    return Some(write!(w, "{v}"))
                }
            }
            None
        })
        .write(&value, w)
}

// pub fn write_compact_value<W: std::io::Write>(writer: W, value: &Value<String>) -> anyhow::Result<()> {
//     write_compact_value_fmt(ToFmtWrite(writer), value)
// }

pub fn write_compact_value_fmt<W: std::fmt::Write>(writer: W, value: &Value<String>) -> anyhow::Result<()> {
    scale_value::stringify::to_writer_custom()
        .compact()
        .format_context(|type_id, w: &mut W| write!(w, "{type_id}"))
        .add_custom_formatter(|v, w: &mut W| scale_value::stringify::custom_formatters::format_hex(v,w))
        .add_custom_formatter(|v, w: &mut W| {
            // don't space unnamed composites over multiple lines if lots of primitive values.
            if let ValueDef::Composite(Composite::Unnamed(vals)) = &v.value {
                let are_primitive = vals.iter().all(|val| matches!(val.value, ValueDef::Primitive(_)));
                if are_primitive {
                    return Some(write!(w, "{v}"))
                }
            }
            None
        })
        .write(value, writer)?;
    Ok(())
}

/// Unwrap the given URl string, returning default Polkadot RPC nodes if not given.
pub fn url_or_polkadot_rpc_nodes(url: Option<&str>) -> Vec<String> {
    // Use our default or built-inPolkadot RPC URLs if not provided.
    let urls = url
        .as_ref()
        .map(|urls| {
            urls.split(',')
                .map(|url| url.to_owned())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| {
            RPC_NODE_URLS
                .iter()
                .map(|url| url.to_string())
                .collect()
        });
    
    urls
}

const RPC_NODE_URLS: [&str; 7] = [
    // "wss://polkadot-rpc.publicnode.com", // bad; can't fetch runtime version.
    "wss://polkadot-public-rpc.blockops.network/ws",
    "wss://polkadot-rpc.dwellir.com",
    "wss://polkadot.api.onfinality.io/public-ws",
    "wss://polkadot.public.curie.radiumblock.co/ws",
    "wss://rockx-dot.w3node.com/polka-public-dot/ws",
    "wss://rpc.ibp.network/polkadot",
    "wss://rpc.dotters.network/polkadot",
    // "wss://dot-rpc.stakeworld.io", // seemed unreliable.
];

/// Wrap a writer to indent any newlines by some amount.
pub struct IndentedWriter<const U: usize, W>(pub W);

impl <const U: usize, W: std::io::Write> std::io::Write for IndentedWriter<U, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // This is dumb and doesn't handle failure to write out buffer.
        for &byte in buf {
            self.0.write(&[byte])?;
            if byte == b'\n' {
                for _ in 0..U {
                    self.0.write(&[b' '])?;
                }
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}