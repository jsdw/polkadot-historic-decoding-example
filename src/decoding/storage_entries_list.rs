use crate::utils::as_decoded;

pub trait StorageEntriesList {
    /// List all of the storage entries available in some metadata.
    fn storage_entries_list(&self) -> impl Iterator<Item = StorageEntry<'_>>;
}

pub struct StorageEntry<'a> {
    pub pallet: &'a str,
    pub entry: &'a str
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
                            pallet: pallet.as_str(),
                            entry: entry.as_str()
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
                            pallet: pallet.name.as_str(),
                            entry: entry.as_str()
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