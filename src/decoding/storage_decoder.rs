use super::storage_type_info::{StorageTypeInfo, StorageHasher};
use frame_metadata::RuntimeMetadata;
use scale_type_resolver::TypeResolver;
use scale_info_legacy::TypeRegistrySet;
use anyhow::{anyhow, bail, Context};

pub type StorageValue = scale_value::Value<String>;
pub type StorageKeys = Vec<StorageKey>;

/// The decoded representation of a storage key.
pub struct StorageKey {
    hash: Vec<u8>,
    value: Option<scale_value::Value<String>>,
    hasher: StorageHasher,
}

/// Is this storage entry iterable? If so, we'll iterate it. If not, we can just retrieve the single entry.
pub fn is_iterable(pallet_name: &str, storage_entry: &str, metadata: &RuntimeMetadata) -> anyhow::Result<bool> {
    match metadata {
        RuntimeMetadata::V8(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V9(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V10(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V11(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V12(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V13(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V14(m) => is_iterable_inner(pallet_name, storage_entry, m),
        RuntimeMetadata::V15(m) => is_iterable_inner(pallet_name, storage_entry, m),
        _ => bail!("Only metadata V8 - V15 is supported")
    }
}

/// Decode the bytes representing some storage key. Here, we expect all of the key bytes including the hashed pallet name and storage entry.
pub fn decode_storage_keys(pallet_name: &str, storage_entry: &str, bytes: &[u8], metadata: &RuntimeMetadata, historic_types: &TypeRegistrySet) -> anyhow::Result<StorageKeys> {
    match metadata {
        RuntimeMetadata::V8(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V9(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V10(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V11(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V12(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V13(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V14(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, &m.types),
        RuntimeMetadata::V15(m) => decode_storage_keys_inner(pallet_name, storage_entry, bytes, m, &m.types),
        _ => bail!("Only metadata V8 - V15 is supported")
    }
}

/// Decode the bytes representing some storage value.
pub fn decode_storage_value(pallet_name: &str, storage_entry: &str, bytes: &[u8], metadata: &RuntimeMetadata, historic_types: &TypeRegistrySet) -> anyhow::Result<StorageValue> {
    match metadata {
        RuntimeMetadata::V8(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V9(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V10(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V11(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V12(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V13(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, historic_types),
        RuntimeMetadata::V14(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, &m.types),
        RuntimeMetadata::V15(m) => decode_storage_value_inner(pallet_name, storage_entry, bytes, m, &m.types),
        _ => bail!("Only metadata V8 - V15 is supported")
    }
}

fn is_iterable_inner<Info>(pallet_name: &str, storage_entry: &str, info: &Info) -> anyhow::Result<bool> 
where
    Info: StorageTypeInfo
{
    let storage_info = info.get_storage_info(pallet_name, storage_entry)
        .with_context(|| "Cannot find storage entry")?;

    Ok(!storage_info.keys.is_empty())
}

fn decode_storage_keys_inner<Info, Resolver>(pallet_name: &str, storage_entry: &str, bytes: &[u8], info: &Info, type_resolver: &Resolver) -> anyhow::Result<StorageKeys>
where
    Info: StorageTypeInfo,
    Info::TypeId: Clone + core::fmt::Display + core::fmt::Debug + Send + Sync + 'static,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let storage_info = info.get_storage_info(pallet_name, storage_entry)
        .with_context(|| "Cannot find storage entry")?;

    let cursor = &mut &*bytes;

    strip_bytes(cursor, 16)
        .with_context(|| "Cannot strip the pallet and storage entry prefix from the storage key")?;

    let decoded: anyhow::Result<StorageKeys> = storage_info.keys.into_iter().map(|key: super::storage_type_info::StorageKey<<Info as StorageTypeInfo>::TypeId>| {
        let hasher = key.hasher;
        match &hasher {
            StorageHasher::Blake2_128 |
            StorageHasher::Twox128 => {
                let hash = strip_bytes(cursor, 16)?;
                Ok(StorageKey { hash, hasher, value: None })
            },
            StorageHasher::Blake2_256 |
            StorageHasher::Twox256 => {
                let hash = strip_bytes(cursor, 32)?;
                Ok(StorageKey { hash, hasher, value: None })
            },
            StorageHasher::Blake2_128Concat => {
                let hash = strip_bytes(cursor, 16)?;
                let value = scale_value::scale::decode_as_type(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Blake2_128Concat storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash, hasher, value: Some(value) })
            },
            StorageHasher::Twox64Concat => {
                let hash = strip_bytes(cursor, 8)?;
                let value = scale_value::scale::decode_as_type(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Twox64Concat storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash, hasher, value: Some(value) })
            },
            StorageHasher::Identity => {
                let value = scale_value::scale::decode_as_type(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Identity storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash: Vec::new(), hasher, value: Some(value) })
            },
        }
    }).collect();

    decoded
}

fn decode_storage_value_inner<Info, Resolver>(pallet_name: &str, storage_entry: &str, bytes: &[u8], info: &Info, type_resolver: &Resolver) -> anyhow::Result<StorageValue>
where
    Info: StorageTypeInfo,
    Info::TypeId: Clone + core::fmt::Display + core::fmt::Debug + Send + Sync + 'static,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let storage_info = info.get_storage_info(pallet_name, storage_entry)
        .with_context(|| "Cannot find storage entry")?;

    let value_id = storage_info.value_id;

    let cursor = &mut &*bytes;

    let decoded = scale_value::scale::decode_as_type(cursor, value_id, type_resolver)
        .with_context(|| "Cannot decode storage value")?
        .map_context(|type_id| type_id.to_string());

    Ok(decoded)
}

fn strip_bytes(cursor: &mut &[u8], num: usize) -> anyhow::Result<Vec<u8>> {
    let bytes = cursor
        .get(..num)
        .ok_or_else(|| anyhow!("Cannot get {num} bytes from cursor; not enough input"))?
        .to_vec();

    *cursor = &cursor[num..];
    Ok(bytes)
}