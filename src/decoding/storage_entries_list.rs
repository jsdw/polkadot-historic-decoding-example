use frame_metadata::RuntimeMetadata;
use anyhow::bail;
use std::borrow::Cow;
use std::collections::VecDeque;

use crate::utils::as_decoded;

/// fetch the list of available storage entries from some metadata.
pub fn get_storage_entries(metadata: &RuntimeMetadata) -> anyhow::Result<VecDeque<StorageEntry<'static>>> {
   let entries = match metadata {
        RuntimeMetadata::V8(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V9(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V10(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V11(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V12(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V13(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V14(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        RuntimeMetadata::V15(m) => m.storage_entries_list().map(|e| e.into_owned()).collect(),
        _ => bail!("Only metadata V8-V15 is supported")
    };
    Ok(entries)
}

pub trait StorageEntriesList {
    /// List all of the storage entries available in some metadata.
    fn storage_entries_list(&self) -> impl Iterator<Item = StorageEntry<'_>>;
}

#[derive(Debug,Clone)]
pub struct StorageEntry<'a> {
    pub pallet: Cow<'a, str>,
    pub entry: Cow<'a, str>
}

impl <'a> StorageEntry<'a> {
    pub fn into_owned(self) -> StorageEntry<'static> {
        StorageEntry {
            pallet: Cow::Owned(self.pallet.into_owned()),
            entry: Cow::Owned(self.entry.into_owned())
        }
    }
}

macro_rules! impl_storage_entries_list_for_v8_to_v12 {
    ($path:path) => {
        impl StorageEntriesList for $path {
            fn storage_entries_list(&self) -> impl Iterator<Item = StorageEntry<'_>> {
                let mut output = vec![];
                
                for module in as_decoded(&self.modules) {
                    let Some(storage) = &module.storage else { continue };
                    let pallet = as_decoded(&module.name);
                    let storage = as_decoded(storage);
                    let entries = as_decoded(&storage.entries);
        
                    for entry_meta in entries {
                        let entry = as_decoded(&entry_meta.name);
                        output.push(StorageEntry {
                            pallet: Cow::Borrowed(pallet.as_str()),
                            entry: Cow::Borrowed(entry.as_str())
                        })
                    }
                }
                output.into_iter()
            }
        }
    }
}

impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v8::RuntimeMetadataV8);
impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v9::RuntimeMetadataV9);
impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v10::RuntimeMetadataV10);
impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v11::RuntimeMetadataV11);
impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v12::RuntimeMetadataV12);
impl_storage_entries_list_for_v8_to_v12!(frame_metadata::v13::RuntimeMetadataV13);

macro_rules! impl_storage_entries_list_for_v14_to_v15 {
    ($path:path) => {
        impl StorageEntriesList for $path {
            fn storage_entries_list(&self) -> impl Iterator<Item = StorageEntry<'_>> {
                let mut output = vec![];
                
                for pallet in &self.pallets {
                    let Some(storage) = &pallet.storage else { continue };

                    for entry_meta in &storage.entries {
                        let entry = &entry_meta.name;
                        output.push(StorageEntry {
                            pallet: Cow::Borrowed(pallet.name.as_str()),
                            entry: Cow::Borrowed(entry.as_str())
                        })
                    }
                }
                output.into_iter()
            }
        }
    }
}

impl_storage_entries_list_for_v14_to_v15!(frame_metadata::v14::RuntimeMetadataV14);
impl_storage_entries_list_for_v14_to_v15!(frame_metadata::v15::RuntimeMetadataV15);