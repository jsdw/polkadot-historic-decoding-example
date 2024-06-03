use frame_metadata::decode_different::DecodeDifferent;
use scale_info_legacy::LookupName;
use anyhow::{anyhow, bail};

/// This is implemented for all metadatas exposed from `frame_metadata` and is respondible for extracting the
/// type IDs that we need in order to decode extrinsics.
pub trait ExtrinsicTypeInfo {
    type TypeId;
    // Get the type IDs for each of the arguments in the call data.
    fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>>;
    // Get the information needed to decode the extrinsic signature bytes.
    fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>>;
}

pub struct Arg<TypeId> {
    pub name: String,
    pub id: TypeId,
}
pub struct ExtrinsicSignatureInfo<TypeId> {
    pub address_id: TypeId,
    pub signature_id: TypeId,
    pub signed_extension_ids: Vec<Arg<TypeId>>
}

macro_rules! impl_call_arg_ids_body_for_v8_to_v11 {
    ($self:ident, $pallet_index:ident, $call_index:ident) => {{
        let modules = as_decoded(&$self.modules);

        let m = modules
            .iter()
            .filter(|m| m.calls.is_some())
            .nth($pallet_index as usize)
            .ok_or_else(|| anyhow!("Couldn't find pallet with index {}", $pallet_index))?;

        let m_name = as_decoded(&m.name);

        let calls = m.calls
            .as_ref()
            .ok_or_else(|| anyhow!("No calls in pallet {m_name} (index {})", $pallet_index))?;

        let calls = as_decoded(calls);

        let call = calls
            .get($call_index as usize)
            .ok_or_else(|| anyhow!("Could not find call with index {} in pallet {m_name} (index {})", $call_index, $pallet_index))?;

        let args = as_decoded(&call.arguments);

        args.iter().map(|a| {
            let ty = as_decoded(&a.ty);
            let id = LookupName::parse(ty).map_err(|e| anyhow!("Could not parse type name {ty}: {e}"))?;
            Ok(Arg { id, name: as_decoded(&a.name).to_owned() })
        }).collect()
    }}
}

macro_rules! impl_for_v8_to_v10 {
    ($path:path) => {
        impl ExtrinsicTypeInfo for $path {
            type TypeId = LookupName;
            fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>> {
                impl_call_arg_ids_body_for_v8_to_v11!(self, pallet_index, call_index)
            }
            fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>> {
                Ok(ExtrinsicSignatureInfo {
                    address_id: LookupName::parse("hardcoded::ExtrinsicAddress").unwrap(),
                    signature_id: LookupName::parse("hardcoded::ExtrinsicSignature").unwrap(),
                    signed_extension_ids: vec![
                        Arg {
                            name: "ExtrinsicSignedExtensions".to_owned(),
                            id: LookupName::parse("hardcoded::ExtrinsicSignedExtensions").unwrap()
                        }
                    ]
                })
            }
        }
    }
}

impl_for_v8_to_v10!(frame_metadata::v8::RuntimeMetadataV8);
impl_for_v8_to_v10!(frame_metadata::v9::RuntimeMetadataV9);
impl_for_v8_to_v10!(frame_metadata::v10::RuntimeMetadataV10);

impl ExtrinsicTypeInfo for frame_metadata::v11::RuntimeMetadataV11 {
    type TypeId = LookupName;
    fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>> {
        impl_call_arg_ids_body_for_v8_to_v11!(self, pallet_index, call_index)

    }
    fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>> {
        // In V11 metadata we start exposing signed extension names, so we use those directly instead of
        // a hardcoded ExtrinsicSignedExtensions type that the user is expected to define.
        let signed_extension_ids = self.extrinsic.signed_extensions.iter().map(|e| {
            let signed_ext_name = as_decoded(e);
            let signed_ext_id = LookupName::parse(signed_ext_name)
                .map_err(|e| anyhow!("Could not parse type name {signed_ext_name}: {e}"))?;

            Ok(Arg { id: signed_ext_id, name: signed_ext_name.clone() })
        }).collect::<Result<Vec<_>,anyhow::Error>>()?;

        Ok(ExtrinsicSignatureInfo {
            address_id: LookupName::parse("hardcoded::ExtrinsicAddress").unwrap(),
            signature_id: LookupName::parse("hardcoded::ExtrinsicSignature").unwrap(),
            signed_extension_ids
        })
    }
}

macro_rules! impl_for_v12_to_v13 {
    ($path:path) => {
        impl ExtrinsicTypeInfo for $path {
            type TypeId = LookupName;
            fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>> {
                let modules = as_decoded(&self.modules);

                let m = modules
                    .iter()
                    .find(|m| m.index == pallet_index)
                    .ok_or_else(|| anyhow!("Couldn't find pallet with index {pallet_index}"))?;

                let m_name = as_decoded(&m.name);

                let calls = m.calls
                    .as_ref()
                    .ok_or_else(|| anyhow!("No calls in pallet {m_name}"))?;

                let calls = as_decoded(calls);

                let call = calls
                    .get(call_index as usize)
                    .ok_or_else(|| anyhow!("Could not find call with index {call_index} in pallet {m_name}"))?;

                let args = as_decoded(&call.arguments);

                args.iter().map(|a| {
                    let ty = as_decoded(&a.ty);
                    let id = LookupName::parse(ty).map_err(|e| anyhow!("Could not parse type name {ty}: {e}"))?;
                    Ok(Arg { id, name: as_decoded(&a.name).to_owned() })
                }).collect()
            }
            fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>> {
                // In V12 metadata we are exposing signed extension names, so we use those directly instead of
                // a hardcoded ExtrinsicSignedExtensions type that the user is expected to define.
                let signed_extension_ids = self.extrinsic.signed_extensions.iter().map(|e| {
                    let signed_ext_name = as_decoded(e);
                    let signed_ext_id = LookupName::parse(signed_ext_name)
                        .map_err(|e| anyhow!("Could not parse type name {signed_ext_name}: {e}"))?;

                    Ok(Arg { id: signed_ext_id, name: signed_ext_name.clone() })
                }).collect::<Result<Vec<_>,anyhow::Error>>()?;

                Ok(ExtrinsicSignatureInfo {
                    address_id: LookupName::parse("hardcoded::ExtrinsicAddress").unwrap(),
                    signature_id: LookupName::parse("hardcoded::ExtrinsicSignature").unwrap(),
                    signed_extension_ids
                })
            }
        }
    }
}

impl_for_v12_to_v13!(frame_metadata::v12::RuntimeMetadataV12);
impl_for_v12_to_v13!(frame_metadata::v13::RuntimeMetadataV13);

macro_rules! impl_call_arg_ids_body_for_v14_to_v15 {
    ($self:ident, $pallet_index:ident, $call_index:ident) => {{
        let pallet = $self.pallets
            .iter()
            .find(|p| p.index == $pallet_index)
            .ok_or_else(|| anyhow!("Couldn't find pallet with index {}", $pallet_index))?;

        let pallet_name = &pallet.name;

        let calls_id = pallet.calls.as_ref()
            .ok_or_else(|| anyhow!("No calls in pallet {pallet_name}"))?
            .ty.id;

        let calls_ty = $self.types.resolve(calls_id)
            .ok_or_else(|| anyhow!("Could not find calls type for {pallet_name} in the type registry"))?;

        let calls_enum = match &calls_ty.type_def {
            scale_info::TypeDef::Variant(v) => v,
            _ => bail!("Calls type in {} should be a variant type, but isn't", pallet.name)
        };

        let call_variant = calls_enum.variants
            .iter()
            .find(|v| v.index == $call_index)
            .ok_or_else(|| anyhow!("Could not find call with index {} in pallet {pallet_name}", $call_index))?;

        Ok(call_variant.fields.iter().map(|f| Arg { id: f.ty.id, name: f.name.as_ref().unwrap().to_owned() }).collect())
    }}
}

impl ExtrinsicTypeInfo for frame_metadata::v14::RuntimeMetadataV14 {
    type TypeId = u32;
    fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>> {
        impl_call_arg_ids_body_for_v14_to_v15!(self, pallet_index, call_index)
    }
    fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>> {
        let signed_extension_ids = self.extrinsic.signed_extensions.iter().map(|e| {
            Arg { id: e.ty.id, name: e.identifier.clone() }
        }).collect();

        let ext_type_ids = ExtrinsicPartTypeIds::new(self)?;

        Ok(ExtrinsicSignatureInfo {
            address_id: ext_type_ids.address,
            signature_id: ext_type_ids.signature,
            signed_extension_ids
        })
    }
}

impl ExtrinsicTypeInfo for frame_metadata::v15::RuntimeMetadataV15 {
    type TypeId = u32;
    fn get_call_argument_ids(&self, pallet_index: u8, call_index: u8) -> anyhow::Result<Vec<Arg<Self::TypeId>>> {
        impl_call_arg_ids_body_for_v14_to_v15!(self, pallet_index, call_index)
    }
    fn get_signature_info(&self) -> anyhow::Result<ExtrinsicSignatureInfo<Self::TypeId>> {
        let signed_extension_ids = self.extrinsic.signed_extensions.iter().map(|e| {
            Arg { id: e.ty.id, name: e.identifier.clone() }
        }).collect();

        Ok(ExtrinsicSignatureInfo {
            address_id: self.extrinsic.address_ty.id,
            signature_id: self.extrinsic.signature_ty.id,
            signed_extension_ids
        })
    }
}

/// A utility function to unwrap the `DecodeDifferent` enum in earlier metadata versions.
fn as_decoded<A, B>(item: &DecodeDifferent<A, B>) -> &B {
    match item {
        DecodeDifferent::Encode(_a) => panic!("Expecting decoded data"),
        DecodeDifferent::Decoded(b) => b,
    }
}

/// The type IDs extracted from V14 metadata that represent the
/// generic type parameters passed to the `UncheckedExtrinsic` from
/// the substrate-based chain.
struct ExtrinsicPartTypeIds {
    address: u32,
    signature: u32,
}

impl ExtrinsicPartTypeIds {
    /// Extract the generic type parameters IDs from the extrinsic type.
    fn new(metadata: &frame_metadata::v14::RuntimeMetadataV14) -> anyhow::Result<Self> {
        use std::collections::HashMap;

        const ADDRESS: &str = "Address";
        const SIGNATURE: &str = "Signature";

        let extrinsic_id = metadata.extrinsic.ty.id;
        let Some(extrinsic_ty) = metadata.types.resolve(extrinsic_id) else {
            bail!("Could not find extrinsic type with ID {extrinsic_id}")
        };

        let params: HashMap<_, _> = extrinsic_ty
            .type_params
            .iter()
            .map(|ty_param| {
                let Some(ty) = ty_param.ty else {
                    bail!("Could not find required type param on Extrinsic type: {}", ty_param.name);
                };

                Ok((ty_param.name.as_str(), ty.id))
            })
            .collect::<Result<_, _>>()?;

        let Some(address) = params.get(ADDRESS) else {
            bail!("Could not find required type param on Extrinsic type: {ADDRESS}");
        };
        let Some(signature) = params.get(SIGNATURE) else {
            bail!("Could not find required type param on Extrinsic type: {SIGNATURE}");
        };

        Ok(ExtrinsicPartTypeIds {
            address: *address,
            signature: *signature,
        })
    }
}