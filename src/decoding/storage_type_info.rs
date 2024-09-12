use scale_info_legacy::LookupName;
use frame_metadata::{decode_different::DecodeDifferent, RuntimeMetadata};
use anyhow::{Context, anyhow, bail};
use crate::utils::as_decoded;

/// Is this storage entry iterable? If so, we'll iterate it. If not, we can just retrieve the single entry.
pub fn is_iterable(pallet_name: &str, storage_entry: &str, metadata: &RuntimeMetadata) -> anyhow::Result<bool> {
    fn inner<Info: StorageTypeInfo>(pallet_name: &str, storage_entry: &str, info: &Info) -> anyhow::Result<bool> {
        let storage_info = info.get_storage_info(pallet_name, storage_entry)
            .with_context(|| "Cannot find storage entry")?;
        Ok(!storage_info.keys.is_empty())
    }

    match metadata {
        RuntimeMetadata::V8(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V9(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V10(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V11(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V12(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V13(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V14(m) => inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V15(m) => inner(pallet_name, storage_entry, m),
        _ => bail!("Only metadata V8 - V15 is supported")
    }
}

/// This is implemented for all metadatas exposed from `frame_metadata` and is responsible for extracting the
/// type IDs and related info needed to decode storage entries.
pub trait StorageTypeInfo {
    type TypeId;
    /// Get the information needed to decode a specific storage entry key/value.
    fn get_storage_info(&self, pallet_name: &str, storage_entry: &str) -> anyhow::Result<StorageInfo<Self::TypeId>>;
}

#[derive(Debug)]
pub struct StorageInfo<TypeId> {
    /// No entries if a plain storage entry, or N entries for N maps.
    pub keys: Vec<StorageKey<TypeId>>,
    /// The type of the values.
    pub value_id: TypeId,
}

#[derive(Debug)]
pub struct StorageKey<TypeId> {
    /// How is this key hashed?
    pub hasher: StorageHasher,
    /// The type of the key.
    pub key_id: TypeId,
}

/// Hasher used by storage maps
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum StorageHasher {
	/// 128-bit Blake2 hash.
	Blake2_128,
	/// 256-bit Blake2 hash.
	Blake2_256,
    /// Multiple 128-bit Blake2 hashes concatenated.
	Blake2_128Concat,
	/// 128-bit XX hash.
	Twox128,
	/// 256-bit XX hash.
	Twox256,
	/// 64-bit XX hashes concatentation.
	Twox64Concat,
	/// Identity hashing (no hashing).
	Identity,
}

macro_rules! impl_storage_type_info_for_v8_to_v12 {
    ($path:path, $name:ident, $to_storage_hasher:ident) => {
        const _: () = {
            use $path as path;
            impl StorageTypeInfo for path :: $name {
                type TypeId = LookupName;
            
                fn get_storage_info(&self, pallet_name: &str, storage_entry: &str) -> anyhow::Result<StorageInfo<Self::TypeId>> {
                    let modules = as_decoded(&self.modules);
            
                    let m = modules
                        .iter()
                        .find(|m| as_decoded(&m.name) == pallet_name)
                        .ok_or_else(|| anyhow!("Couldn't find pallet with name {pallet_name}"))?;
            
                    let storages = m.storage
                        .as_ref()
                        .map(|s| as_decoded(s))
                        .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entries"))?;
            
                    let storage = as_decoded(&storages.entries)
                        .iter()
                        .find(|s| as_decoded(&s.name) == storage_entry)
                        .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entry called {storage_entry}"))?;
            
                    match &storage.ty {
                        path::StorageEntryType::Plain(ty) => {
                            let value_id = decode_lookup_name_or_err(ty, pallet_name)?;
                            Ok(StorageInfo {
                                keys: vec![],
                                value_id
                            })
                        },
                        path::StorageEntryType::Map { hasher, key, value, .. } => {
                            let key_id = decode_lookup_name_or_err(key, pallet_name)?;
                            let hasher = $to_storage_hasher(hasher);
                            let value_id = decode_lookup_name_or_err(value, pallet_name)?;
                            Ok(StorageInfo { 
                                keys: vec![StorageKey { hasher, key_id }],
                                value_id
                            })
                        },
                        path::StorageEntryType::DoubleMap { hasher, key1, key2, value, key2_hasher } => {
                            let key1_id = decode_lookup_name_or_err(key1, pallet_name)?;
                            let key1_hasher = $to_storage_hasher(hasher);
                            let key2_id = decode_lookup_name_or_err(key2, pallet_name)?;
                            let key2_hasher = $to_storage_hasher(key2_hasher);
                            let value_id = decode_lookup_name_or_err(value, pallet_name)?;
                            Ok(StorageInfo { 
                                keys: vec![
                                    StorageKey { hasher: key1_hasher, key_id: key1_id },
                                    StorageKey { hasher: key2_hasher, key_id: key2_id },
                                ],
                                value_id
                            })
                        }
                    }
                }
            }
        };
    }
}

impl_storage_type_info_for_v8_to_v12!(frame_metadata::v8, RuntimeMetadataV8, to_storage_hasher_v8);
impl_storage_type_info_for_v8_to_v12!(frame_metadata::v9, RuntimeMetadataV9, to_storage_hasher_v9);
impl_storage_type_info_for_v8_to_v12!(frame_metadata::v10, RuntimeMetadataV10, to_storage_hasher_v10);
impl_storage_type_info_for_v8_to_v12!(frame_metadata::v11, RuntimeMetadataV11, to_storage_hasher_v11);
impl_storage_type_info_for_v8_to_v12!(frame_metadata::v12, RuntimeMetadataV12, to_storage_hasher_v12);

impl StorageTypeInfo for frame_metadata::v13::RuntimeMetadataV13 {
    type TypeId = LookupName;

    fn get_storage_info(&self, pallet_name: &str, storage_entry: &str) -> anyhow::Result<StorageInfo<Self::TypeId>> {
        let modules = as_decoded(&self.modules);
            
        let m = modules
            .iter()
            .find(|m| as_decoded(&m.name) == pallet_name)
            .ok_or_else(|| anyhow!("Couldn't find pallet with name {pallet_name}"))?;

        let storages = m.storage
            .as_ref()
            .map(|s| as_decoded(s))
            .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entries"))?;

        let storage = as_decoded(&storages.entries)
            .iter()
            .find(|s| as_decoded(&s.name) == storage_entry)
            .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entry called {storage_entry}"))?;

        match &storage.ty {
            frame_metadata::v13::StorageEntryType::Plain(ty) => {
                let value_id = decode_lookup_name_or_err(ty, pallet_name)?;
                Ok(StorageInfo {
                    keys: vec![],
                    value_id
                })
            },
            frame_metadata::v13::StorageEntryType::Map { hasher, key, value, .. } => {
                let key_id = decode_lookup_name_or_err(key, pallet_name)?;
                let hasher = to_storage_hasher_v13(hasher);
                let value_id = decode_lookup_name_or_err(value, pallet_name)?;
                Ok(StorageInfo { 
                    keys: vec![StorageKey { hasher, key_id }],
                    value_id
                })
            },
            frame_metadata::v13::StorageEntryType::DoubleMap { hasher, key1, key2, value, key2_hasher } => {
                let key1_id = decode_lookup_name_or_err(key1, pallet_name)?;
                let key1_hasher = to_storage_hasher_v13(hasher);
                let key2_id = decode_lookup_name_or_err(key2, pallet_name)?;
                let key2_hasher = to_storage_hasher_v13(key2_hasher);
                let value_id = decode_lookup_name_or_err(value, pallet_name)?;
                Ok(StorageInfo { 
                    keys: vec![
                        StorageKey { hasher: key1_hasher, key_id: key1_id },
                        StorageKey { hasher: key2_hasher, key_id: key2_id },
                    ],
                    value_id
                })
            },
            frame_metadata::v13::StorageEntryType::NMap { keys, hashers, value } => {
                let keys = as_decoded(keys);
                let hashers = as_decoded(hashers);
                let value_id = decode_lookup_name_or_err(value, pallet_name)?;

                // If one hasher and lots of keys then hash each key the same.
                // If one hasher per key then unique hasher per key.
                // Else, there's some error.
                let keys: anyhow::Result<Vec<_>> = if hashers.len() == 1 {
                    let hasher = to_storage_hasher_v13(&hashers[0]);
                    keys.iter()
                        .map(|key| {
                            let key_id = lookup_name_or_err(key, pallet_name)?;
                            Ok(StorageKey { hasher, key_id })
                        })
                        .collect()
                } else if hashers.len() == keys.len() {
                    keys.iter()
                        .zip(hashers)
                        .map(|(key, hasher)| {
                            let hasher = to_storage_hasher_v13(hasher);
                            let key_id = lookup_name_or_err(key, pallet_name)?;
                            Ok(StorageKey { hasher, key_id })
                        })
                        .collect()
                } else {
                    Err(anyhow!("Hashers and key count should match, but got {} hashers and {} keys", hashers.len(), keys.len()))
                };

                Ok(StorageInfo { 
                    keys: keys?,
                    value_id
                })
            }
        }
    }
}

macro_rules! impl_storage_type_info_for_v14_to_v15 {
    ($path:path, $name:ident, $to_storage_hasher:ident) => {
        const _: () = {
            use $path as path;
            impl StorageTypeInfo for path :: $name {
                type TypeId = u32;
                fn get_storage_info(&self, pallet_name: &str, storage_entry: &str) -> anyhow::Result<StorageInfo<Self::TypeId>> {
                    let pallet = self.pallets
                        .iter()
                        .find(|p| &p.name == pallet_name)
                        .ok_or_else(|| anyhow!("Couldn't find pallet with name {pallet_name}"))?;
            
                    let storages = pallet.storage
                        .as_ref()
                        .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entries"))?;
            
                    let storage = storages.entries
                        .iter()
                        .find(|e| &e.name == storage_entry)
                        .ok_or_else(|| anyhow!("Pallet {pallet_name} has no storage entry called {storage_entry}"))?;
            
                    match &storage.ty {
                        path::StorageEntryType::Plain(value) => {
                            Ok(StorageInfo { 
                                keys: vec![], 
                                value_id: value.id 
                            })
                        },
                        path::StorageEntryType::Map { hashers, key, value } => {
                            let value_id = value.id;
                            let key_id = key.id;
                            let key_ty = self.types
                                .resolve(key_id)
                                .ok_or_else(|| anyhow!("Cannot find type {key_id} of storage entry {pallet_name}.{storage_entry}"))?;
            
                            if let scale_info::TypeDef::Tuple(tuple) = &key_ty.type_def {
                                if hashers.len() == 1 {
                                    // Multiple keys but one hasher; use same hasher for every key
                                    let hasher = $to_storage_hasher(&hashers[0]);
                                    Ok(StorageInfo { 
                                        keys: tuple.fields.iter().map(|f| StorageKey { hasher, key_id: f.id }).collect(), 
                                        value_id
                                    })
                                } else if hashers.len() == tuple.fields.len() {
                                    // One hasher per key
                                    let keys = tuple.fields.iter().zip(hashers).map(|(field, hasher)| {
                                        StorageKey {
                                            hasher: $to_storage_hasher(hasher),
                                            key_id: field.id
                                        }
                                    }).collect();
                                    Ok(StorageInfo { 
                                        keys, 
                                        value_id
                                    })
                                } else {
                                    // Hasher and key mismatch
                                    Err(anyhow!("Hasher & key mismatch in storage entry {pallet_name}.{storage_entry}: {} hashers and {} keys", hashers.len(), tuple.fields.len()))
                                }
                            } else if hashers.len() == 1 {
                                // One key, one hasher.
                                Ok(StorageInfo { 
                                    keys: vec![
                                        StorageKey { hasher: $to_storage_hasher(&hashers[0]), key_id }
                                    ], 
                                    value_id
                                })
                            } else {
                                // Multiple hashers but only one key; error.
                                Err(anyhow!("Hasher & key mismatch in storage entry {pallet_name}.{storage_entry}: {} hashers and 1 key", hashers.len()))
                            }
                        },
                    }
                }
            }
        };
    }
}

impl_storage_type_info_for_v14_to_v15!(frame_metadata::v14, RuntimeMetadataV14, to_storage_hasher_v14);
impl_storage_type_info_for_v14_to_v15!(frame_metadata::v15, RuntimeMetadataV15, to_storage_hasher_v15);

fn to_storage_hasher_v8(hasher: &frame_metadata::v8::StorageHasher) -> StorageHasher {
    match hasher {
        frame_metadata::v8::StorageHasher::Blake2_128 => StorageHasher::Blake2_128,
        frame_metadata::v8::StorageHasher::Blake2_256 => StorageHasher::Blake2_256,
        frame_metadata::v8::StorageHasher::Twox128 => StorageHasher::Twox128,
        frame_metadata::v8::StorageHasher::Twox256 => StorageHasher::Twox256,
        frame_metadata::v8::StorageHasher::Twox64Concat => StorageHasher::Twox64Concat,
    }
}
fn to_storage_hasher_v9(hasher: &frame_metadata::v9::StorageHasher) -> StorageHasher {
    match hasher {
        frame_metadata::v9::StorageHasher::Blake2_128 => StorageHasher::Blake2_128,
        frame_metadata::v9::StorageHasher::Blake2_128Concat => StorageHasher::Blake2_128Concat,
        frame_metadata::v9::StorageHasher::Blake2_256 => StorageHasher::Blake2_256,
        frame_metadata::v9::StorageHasher::Twox128 => StorageHasher::Twox128,
        frame_metadata::v9::StorageHasher::Twox256 => StorageHasher::Twox256,
        frame_metadata::v9::StorageHasher::Twox64Concat => StorageHasher::Twox64Concat,
    }
}
fn to_storage_hasher_v10(hasher: &frame_metadata::v10::StorageHasher) -> StorageHasher {
    match hasher {
        frame_metadata::v10::StorageHasher::Blake2_128 => StorageHasher::Blake2_128,
        frame_metadata::v10::StorageHasher::Blake2_128Concat => StorageHasher::Blake2_128Concat,
        frame_metadata::v10::StorageHasher::Blake2_256 => StorageHasher::Blake2_256,
        frame_metadata::v10::StorageHasher::Twox128 => StorageHasher::Twox128,
        frame_metadata::v10::StorageHasher::Twox256 => StorageHasher::Twox256,
        frame_metadata::v10::StorageHasher::Twox64Concat => StorageHasher::Twox64Concat,
    }
}

macro_rules! to_latest_storage_hasher {
    ($ident:ident, $path:path) => {
        fn $ident(hasher: &$path) -> StorageHasher {
            match hasher {
                <$path>::Blake2_128 => StorageHasher::Blake2_128,
                <$path>::Blake2_128Concat => StorageHasher::Blake2_128Concat,
                <$path>::Blake2_256 => StorageHasher::Blake2_256,
                <$path>::Twox128 => StorageHasher::Twox128,
                <$path>::Twox256 => StorageHasher::Twox256,
                <$path>::Twox64Concat => StorageHasher::Twox64Concat,
                <$path>::Identity => StorageHasher::Identity,
            }
        }
    }
}

to_latest_storage_hasher!(to_storage_hasher_v11, frame_metadata::v11::StorageHasher);
to_latest_storage_hasher!(to_storage_hasher_v12, frame_metadata::v12::StorageHasher);
to_latest_storage_hasher!(to_storage_hasher_v13, frame_metadata::v13::StorageHasher);
to_latest_storage_hasher!(to_storage_hasher_v14, frame_metadata::v14::StorageHasher);
to_latest_storage_hasher!(to_storage_hasher_v15, frame_metadata::v15::StorageHasher);

fn decode_lookup_name_or_err(s: &DecodeDifferent<&str, String>, pallet_name: &str) -> anyhow::Result<LookupName> {
    let ty = sanitize_type_name(&as_decoded(s));
    lookup_name_or_err(&ty, pallet_name)
}

fn lookup_name_or_err(ty: &str, pallet_name: &str) -> anyhow::Result<LookupName> {
    let id = LookupName::parse(ty)
        .map_err(|e| anyhow!("Could not parse type name {ty}: {e}"))?
        .in_pallet(pallet_name);
    Ok(id)
}

fn sanitize_type_name(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains('\n') {
        std::borrow::Cow::Owned(name.replace('\n', ""))
    } else {
        std::borrow::Cow::Borrowed(name)
    }
}