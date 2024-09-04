pub mod extrinsic_decoder;
pub mod extrinsic_type_info;
pub mod storage_decoder;
pub mod storage_type_info;
pub mod storage_entries_list;

use scale_info_legacy::{TypeRegistry, LookupName};
use crate::utils::as_decoded;

/// Add runtime call information to our type info given the metadata. This allows us to decode things like
/// utility.batch, which contains inner calls, without having to hardcode the Call info in our types.
pub fn extend_with_metadata_info(types: &mut scale_info_legacy::TypeRegistrySet, metadata: &frame_metadata::RuntimeMetadata) -> anyhow::Result<()> {
    use scale_info_legacy::type_shape::{TypeShape,Field,Variant,VariantDesc};
    use scale_info_legacy::InsertName;

    macro_rules! impl_for_v8_to_v13 {
        ($metadata:ident $($builtin_index:ident)?) => {{
            let mut new_types = TypeRegistry::empty();

            let modules = as_decoded(&$metadata.modules);
            let modules_iter = modules
                .iter()
                .filter(|m| m.calls.is_some())
                .enumerate();

            let mut call_module_variants: Vec<Variant> = vec![];
            let mut event_module_variants: Vec<Variant> = vec![];
            for (m_idx, module) in modules_iter {
                // For v12 and v13 metadata, there is a buildin index.
                // If we pass an ident as second arg to this macro, we'll trigger
                // using that builtin index instead.
                $(
                    let $builtin_index = true;
                    let m_idx = if $builtin_index {
                        module.index as usize
                    } else {
                        m_idx
                    };
                )?

                let module_name = as_decoded(&module.name);

                //// 1. Add calls to the type registry
                if let Some(calls) = &module.calls.as_ref() {
                    let calls = as_decoded(calls);

                    // Iterate over each call in the module and turn into variants:
                    let mut call_variants: Vec<Variant> = vec![];
                    for (c_idx, call) in calls.iter().enumerate() {
                        let call_name = as_decoded(&call.name);
                        let args = as_decoded(&call.arguments)
                            .iter()
                            .map(|arg| {
                                Ok(Field {
                                    name: as_decoded(&arg.name).to_owned(),
                                    value: LookupName::parse(&as_decoded(&arg.ty))?.in_pallet(module_name),
                                })
                            })
                            .collect::<anyhow::Result<_>>()?;

                        call_variants.push(Variant {
                            index: c_idx as u8,
                            name: call_name.clone(),
                            fields: VariantDesc::StructOf(args)
                        });
                    }

                    // Store these call variants in the types:
                    let call_enum_name_str = format!("builtin::module::call::{module_name}");
                    let call_enum_insert_name = InsertName::parse(&call_enum_name_str).unwrap();
                    new_types.insert(call_enum_insert_name, TypeShape::EnumOf(call_variants));

                    // Reference it in the modules enum we're building:
                    let call_enum_lookup_name = LookupName::parse(&call_enum_name_str).unwrap();
                    call_module_variants.push(Variant {
                        index: m_idx as u8,
                        name: module_name.clone(),
                        fields: VariantDesc::TupleOf(vec![call_enum_lookup_name])
                    });
                }

                //// 2. Add events to the type registry
                if let Some(events) = &module.event.as_ref() {
                    let events = as_decoded(events);

                    let mut event_variants: Vec<Variant> = vec![];
                    for (e_idx, event)in events.iter().enumerate() {
                        let event_name = as_decoded(&event.name);
                        let args = as_decoded(&event.arguments)
                            .iter()
                            .map(|arg| {
                                Ok(LookupName::parse(&arg)?.in_pallet(module_name))
                            })
                            .collect::<anyhow::Result<_>>()?;

                        event_variants.push(Variant {
                            index: e_idx as u8,
                            name: event_name.clone(),
                            fields: VariantDesc::TupleOf(args)
                        });
                    }

                    // Store event variants in the types:
                    let event_enum_name_str = format!("builtin::module::event::{module_name}");
                    let event_enum_insert_name = InsertName::parse(&event_enum_name_str).unwrap();
                    new_types.insert(event_enum_insert_name, TypeShape::EnumOf(event_variants));

                    // Reference it in the modules enum we're building:
                    let event_enum_lookup_name = LookupName::parse(&event_enum_name_str).unwrap();
                    event_module_variants.push(Variant {
                        index: m_idx as u8,
                        name: module_name.clone(),
                        fields: VariantDesc::TupleOf(vec![event_enum_lookup_name])
                    });
                }
            }

            // Store the module call variants in the types:
            let calls_enum_name_str = "builtin::Call";
            let calls_enum_insert_name = InsertName::parse(&calls_enum_name_str).unwrap();
            new_types.insert(calls_enum_insert_name, TypeShape::EnumOf(call_module_variants));

            // Store the module event variants in the types:
            let events_enum_name_str = "builtin::Event";
            let events_enum_insert_name = InsertName::parse(&events_enum_name_str).unwrap();
            new_types.insert(events_enum_insert_name, TypeShape::EnumOf(event_module_variants));

            // Extend our type registry set with the new types (giving them the lowest priority).
            types.prepend(new_types);
        }}
    }

    match metadata {
        frame_metadata::RuntimeMetadata::V8(m) => impl_for_v8_to_v13!(m),
        frame_metadata::RuntimeMetadata::V9(m) => impl_for_v8_to_v13!(m),
        frame_metadata::RuntimeMetadata::V10(m) => impl_for_v8_to_v13!(m),
        frame_metadata::RuntimeMetadata::V11(m) => impl_for_v8_to_v13!(m),
        frame_metadata::RuntimeMetadata::V12(m) => impl_for_v8_to_v13!(m use_builtin_index),
        frame_metadata::RuntimeMetadata::V13(m) => impl_for_v8_to_v13!(m use_builtin_index),
        _ => {/* do nothing if metadata too old or new */}
    };

    Ok(())
}