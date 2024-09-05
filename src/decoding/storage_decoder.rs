use super::storage_type_info::{StorageTypeInfo, StorageHasher};
use frame_metadata::RuntimeMetadata;
use scale_type_resolver::TypeResolver;
use scale_info_legacy::TypeRegistrySet;
use anyhow::{anyhow, bail, Context};

pub type StorageValue = scale_value::Value<String>;
pub type StorageKeys = Vec<StorageKey>;

/// The decoded representation of a storage key.
pub struct StorageKey {
    pub hash: Vec<u8>,
    pub value: Option<scale_value::Value<String>>,
    pub hasher: StorageHasher,
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

fn decode_storage_keys_inner<Info, Resolver>(pallet_name: &str, storage_entry: &str, bytes: &[u8], info: &Info, type_resolver: &Resolver) -> anyhow::Result<StorageKeys>
where
    Info: StorageTypeInfo,
    Info::TypeId: Clone + core::fmt::Display + core::fmt::Debug + Send + Sync + 'static,
    Resolver: TypeResolver<TypeId = Info::TypeId>,
{
    let storage_info = info.get_storage_info(pallet_name, storage_entry)
        .with_context(|| "Cannot find storage entry")?;

    let cursor = &mut &*bytes;

    let prefix = strip_bytes(cursor, 32)
        .with_context(|| "Cannot strip the pallet and storage entry prefix from the storage key")?;

    // Check that the storage key prefix is what we expect:
    let expected_prefix = {
        let mut v = Vec::<u8>::with_capacity(16);
        v.extend(&sp_crypto_hashing::twox_128(pallet_name.as_bytes()));
        v.extend(&sp_crypto_hashing::twox_128(storage_entry.as_bytes()));
        v
    };
    if prefix != expected_prefix {
        bail!("Storage prefix for {pallet_name}.{storage_entry} does not match expected prefix of {}", hex::encode(expected_prefix))
    }

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
                let value = decode_or_trace(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Blake2_128Concat storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash, hasher, value: Some(value) })
            },
            StorageHasher::Twox64Concat => {
                let hash = strip_bytes(cursor, 8)?;
                let value = decode_or_trace(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Twox64Concat storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash, hasher, value: Some(value) })
            },
            StorageHasher::Identity => {
                let value = decode_or_trace(cursor, key.key_id, type_resolver)
                    .with_context(|| "Cannot decode Identity storage key")?
                    .map_context(|type_id| type_id.to_string());
                Ok(StorageKey { hash: Vec::new(), hasher, value: Some(value) })
            },
        }
    }).collect();

    if !cursor.is_empty() && decoded.is_ok() {
        let decoded = print_storage_key_res(&decoded)?;
        bail!("{} leftover bytes decoding storage keys: {cursor:?}. decoded: {decoded}", cursor.len());
    }

    decoded
}

fn print_storage_key_res(keys: &anyhow::Result<StorageKeys>) -> anyhow::Result<String> {
    match keys {
        Err(e) => Ok(format!("Error: {e}")),
        Ok(keys) => {
            let mut s = String::new();
            write_storage_keys_fmt(&mut s, keys)?;
            Ok(s)
        }
    }
}

pub fn write_storage_keys<W: std::io::Write>(writer: W, keys: &[StorageKey]) -> anyhow::Result<()> {
    let writer = crate::utils::ToFmtWrite(writer);
    write_storage_keys_fmt(writer, keys)
}

pub fn write_storage_keys_fmt<W: std::fmt::Write>(mut writer: W, keys: &[StorageKey]) -> anyhow::Result<()> {
    // Plain entries have no keys:
    if keys.is_empty() {
        write!(&mut writer, "plain")?;
        return Ok(())
    }

    // blake2: AccountId(0x2331) + ident: Foo(123) + blake2:0x23edbfe
    for (idx, key) in keys.into_iter().enumerate() {
        if idx != 0 {
            write!(&mut writer, " + ")?;
        }

        match (key.hasher, &key.value) {
            (StorageHasher::Blake2_128, None) => {
                write!(&mut writer, "blake2_128: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Blake2_256, None) => {
                write!(&mut writer, "blake2_256: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Blake2_128Concat, Some(value)) => {
                write!(&mut writer, "blake2_128_concat: ")?;
                crate::utils::write_compact_value_fmt(&mut writer, &value)?;
            },
            (StorageHasher::Twox128, None) => {
                write!(&mut writer, "twox_128: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Twox256, None) => {
                write!(&mut writer, "twox_256: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Twox64Concat, Some(value)) => {
                write!(&mut writer, "twox64_concat: ")?;
                crate::utils::write_compact_value_fmt(&mut writer, &value)?;
            },
            (StorageHasher::Identity, Some(value)) => {
                write!(&mut writer, "ident: ")?;
                crate::utils::write_compact_value_fmt(&mut writer, &value)?;
            },
            _ => {
                bail!("Invalid storage hasher/value pair")
            }
        }
    }

    Ok(())
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

    let decoded = decode_or_trace(cursor, value_id, type_resolver)
        .with_context(|| format!("Cannot decode storage value"))? // 0x{}", hex::encode(bytes)))?
        .map_context(|type_id| type_id.to_string());

    if !cursor.is_empty() {
        let mut value_string = String::new();
        crate::utils::write_value_fmt(&mut value_string, &decoded)?;
        bail!("{} leftover bytes decoding storage value: {cursor:?}. decoded:\n\n{value_string}", cursor.len());
    }

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

fn decode_or_trace<Resolver, Id>(cursor: &mut &[u8], type_id: Id, types: &Resolver) -> anyhow::Result<scale_value::Value<String>> 
where
    Resolver: TypeResolver<TypeId = Id>,
    Id: core::fmt::Debug + core::fmt::Display + Clone + Send + Sync + 'static
{
    let initial = *cursor;
    match scale_value::scale::decode_as_type(cursor, type_id.clone(), types) {
        Ok(value) => Ok(value.map_context(|id| id.to_string())),
        Err(_e) => {
            // Reset cursor incase it's been consumed by the above call.
            *cursor = initial;
            scale_value::scale::tracing::decode_as_type(cursor, type_id.clone(), types)
                .map(|v| v.map_context(|id| id.to_string()))
                .with_context(|| format!("Failed to decode type with id {type_id}"))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_strip_bytes() {
        let v = vec![0,1,2,3,4,5,6,7,8];
        let cursor = &mut &*v;
        let stripped = strip_bytes(cursor, 4).unwrap();
        assert_eq!(stripped, &[0,1,2,3]);
        assert_eq!(cursor, &[4,5,6,7,8]);
    }
}