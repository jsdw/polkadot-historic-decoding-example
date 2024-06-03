use crate::extrinsic_type_info::ExtrinsicTypeInfo;
use scale_info_legacy::TypeRegistrySet;
use scale_type_resolver::TypeResolver;
use parity_scale_codec::{Decode, Compact};
use anyhow::{bail, Context};
use frame_metadata::RuntimeMetadata;

#[derive(Debug)]
pub enum Extrinsic {
    V4Unsigned {
        call_data: Vec<(String, scale_value::Value)>
    },
    V4Signed {
        address: scale_value::Value,
        signature: scale_value::Value,
        signed_exts: Vec<(String, scale_value::Value)>,
        call_data: Vec<(String, scale_value::Value)>
    }
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
    Info::TypeId: Clone + std::fmt::Display,
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
    Info::TypeId: Clone + std::fmt::Display,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let is_signed = bytes[0] & 0b1000_0000 != 0;
    let cursor = &mut &bytes[1..];

    if is_signed {
        let signature_info = args_info.get_signature_info()?;

        let address = scale_value::scale::decode_as_type(cursor, signature_info.address_id, type_resolver)?
            .remove_context();
        let signature = scale_value::scale::decode_as_type(cursor, signature_info.signature_id, type_resolver)?
            .remove_context();
        let signed_exts = signature_info.signed_extension_ids.into_iter().map(|signed_ext| {
            let decoded_ext = scale_value::scale::decode_as_type(cursor, signed_ext.id, type_resolver)?;
            Ok((signed_ext.name, decoded_ext.remove_context()))
        }).collect::<anyhow::Result<_>>()?;

        let call_data = decode_v4_extrinsic_call_data(cursor, args_info, type_resolver)?;
        Ok(Extrinsic::V4Signed { address, signature, signed_exts, call_data })
    } else {
        let call_data = decode_v4_extrinsic_call_data(cursor, args_info, type_resolver)?;
        Ok(Extrinsic::V4Unsigned { call_data })
    }
}

fn decode_v4_extrinsic_call_data<Info, Resolver>(cursor: &mut &[u8], args_info: &Info, type_resolver: &Resolver) -> anyhow::Result<Vec<(String, scale_value::Value)>>
where
    Info: ExtrinsicTypeInfo,
    Info::TypeId: Clone + std::fmt::Display,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let pallet_index: u8 = Decode::decode(cursor)?;
    let call_index: u8 = Decode::decode(cursor)?;
    let call_args = args_info.get_call_argument_ids(pallet_index, call_index)?;

    let mut call_data = vec![];
    for arg in call_args {
        let value = scale_value::scale::decode_as_type(cursor, arg.id.clone(), type_resolver)
            .with_context(|| format!("Failed to decode type '{}' into a Value", arg.id))?;
        call_data.push((arg.name, value.remove_context()))
    }

    if !cursor.is_empty() {
        bail!("Leftover bytes found during extrinsic decoding");
    }

    Ok(call_data)
}