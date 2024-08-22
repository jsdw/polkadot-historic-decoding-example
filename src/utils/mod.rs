pub mod binary_chopper;
pub mod runner;

use scale_value::{Composite, Value, ValueDef};

/// Write out a pretty Value using `std::io::Write`.
pub fn write_value<W: std::io::Write, T: std::fmt::Display>(w: W, value: &Value<T>) -> core::fmt::Result {
    // Our stdout lock is io::Write but we need fmt::Write below.
    // Ideally we'd about this, but io::Write is std-only among
    // other things, so scale-value uses fmt::Write.
    struct ToFmtWrite<W>(W);
    impl <W: std::io::Write> std::fmt::Write for ToFmtWrite<W> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes()).map(|_| ()).map_err(|_| std::fmt::Error)
        }
    }

    write_value_fmt(ToFmtWrite(w), value, "      ")
}

/// Write out a pretty Value using `std::fmt::Write`.
pub fn write_value_fmt<W: std::fmt::Write, T: std::fmt::Display>(w: W, value: &Value<T>, leading_indent: impl Into<String>) -> core::fmt::Result {
    scale_value::stringify::to_writer_custom()
        .pretty()
        .leading_indent(leading_indent.into())
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