use crate::extrinsic_type_info::ExtrinsicTypeInfo;
use scale_info_legacy::TypeRegistrySet;
use scale_type_resolver::TypeResolver;
use parity_scale_codec::{Decode, Compact};
use anyhow::{bail, Context};
use frame_metadata::RuntimeMetadata;
use subxt::utils::{to_hex, AccountId32};

#[derive(Debug)]
pub enum Extrinsic {
    V4Unsigned {
        call_data: ExtrinsicCallData
    },
    V4Signed {
        address: String,
        signature: String,
        signed_exts: Vec<(String, scale_value::Value)>,
        call_data: ExtrinsicCallData
    }
}

#[derive(Debug)]
pub struct ExtrinsicCallData {
    pub pallet_name: String,
    pub call_name: String,
    pub args: Vec<(String, scale_value::Value)>
}

pub fn decode_extrinsic(bytes: &[u8], metadata: &RuntimeMetadata, historic_types: &TypeRegistrySet) -> anyhow::Result<Extrinsic> {
    match metadata {
        RuntimeMetadata::V8(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V9(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V10(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V11(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V12(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V13(m) => decode_extrinsic_inner(bytes, m, historic_types),
        RuntimeMetadata::V14(m) => decode_extrinsic_inner(bytes, m, &m.types),
        RuntimeMetadata::V15(m) => decode_extrinsic_inner(bytes, m, &m.types),
        _ => bail!("Only metadata V8 - V15 is supported")
    }
}

fn decode_extrinsic_inner<Info, Resolver>(bytes: &[u8], args_info: &Info, type_resolver: &Resolver) -> anyhow::Result<Extrinsic>
where
    Info: ExtrinsicTypeInfo,
    Info::TypeId: Clone + std::fmt::Display + std::fmt::Debug,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let cursor = &mut &*bytes;

    let ext_len = Compact::<u64>::decode(cursor)
        .with_context(|| "Cannot decode the extrinsic length")?.0 as usize;

    let ext_bytes = cursor.get(0..ext_len)
        .with_context(|| "Missing extrinsic bytes")?;

    *cursor = &cursor[ext_len..];

    if ext_bytes.len() < 1 {
        bail!("Missing extrinsic bytes");
    }

    // Decide how to decode the extrinsic based on the version.
    let version = ext_bytes[0] & 0b0111_1111;

    match version {
        4 => decode_v4_extrinsic(ext_bytes, args_info, type_resolver),
        v => bail!("extrinsic version {v} is not supported")
    }
}

fn decode_v4_extrinsic<Info, Resolver>(bytes: &[u8], args_info: &Info, type_resolver: &Resolver) -> anyhow::Result<Extrinsic>
where
    Info: ExtrinsicTypeInfo,
    Info::TypeId: Clone + std::fmt::Display + std::fmt::Debug,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let is_signed = bytes[0] & 0b1000_0000 != 0;
    let cursor = &mut &bytes[1..];

    if is_signed {
        let signature_info = args_info.get_signature_info()?;

        let (_address_value, address_bytes) = with_consumed_bytes(cursor, |cursor| {
            scale_value::scale::decode_as_type(cursor, signature_info.address_id, type_resolver)
                .map(|v| v.remove_context())
        });
        let address = AccountId32(address_bytes.try_into().unwrap()).to_string();

        let (_signature_value, signature_bytes) = with_consumed_bytes(cursor, |cursor| {
            scale_value::scale::decode_as_type(cursor, signature_info.signature_id, type_resolver)
                .map(|v| v.remove_context())
        });
        let signature = to_hex(signature_bytes);

        let signed_exts = signature_info.signed_extension_ids.into_iter().map(|signed_ext| {
            let decoded_ext = scale_value::scale::decode_as_type(cursor, signed_ext.id, type_resolver)?;
            Ok((signed_ext.name, decoded_ext.remove_context()))
        }).collect::<anyhow::Result<Vec<_>>>()?;

        let call_data = decode_v4_extrinsic_call_data(cursor, args_info, type_resolver)?;
        Ok(Extrinsic::V4Signed { address, signature, signed_exts, call_data })
    } else {
        let call_data = decode_v4_extrinsic_call_data(cursor, args_info, type_resolver)?;
        Ok(Extrinsic::V4Unsigned { call_data })
    }
}

fn decode_v4_extrinsic_call_data<Info, Resolver>(cursor: &mut &[u8], args_info: &Info, type_resolver: &Resolver) -> anyhow::Result<ExtrinsicCallData>
where
    Info: ExtrinsicTypeInfo,
    Info::TypeId: Clone + std::fmt::Display + std::fmt::Debug,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let pallet_index: u8 = Decode::decode(cursor)?;
    let call_index: u8 = Decode::decode(cursor)?;
    let extrinsic_info = args_info.get_extrinsic_info(pallet_index, call_index)?;

    let mut args = vec![];
    for arg in extrinsic_info.args {
        let value = scale_value::scale::decode_as_type(cursor, arg.id.clone(), type_resolver)
            .with_context(|| format!("Failed to decode type '{}' into a Value in extrinsic {}.{}", arg.id, extrinsic_info.pallet_name, extrinsic_info.call_name))?;
        args.push((arg.name, value.remove_context()))
    }

    if !cursor.is_empty() {
        bail!("Leftover bytes found during extrinsic decoding");
    }

    Ok(ExtrinsicCallData {
        pallet_name: extrinsic_info.pallet_name,
        call_name: extrinsic_info.call_name,
        args
    })
}

fn with_consumed_bytes<'a, T, F: FnOnce(&mut &[u8]) -> T>(bytes: &mut &'a [u8], f: F) -> (T, &'a [u8]) {
    let initial_bytes = *bytes;
    let res = f(bytes);
    let consumed_bytes = &initial_bytes[0.. initial_bytes.len() - bytes.len()];
    (res,consumed_bytes)
}