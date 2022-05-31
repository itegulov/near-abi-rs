use anyhow::{anyhow, Result};
use near_sdk::{serde_json, Metadata};
use quote::{format_ident, quote};
use schemafy_lib::Expander;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs};

/// Configuration options for ABI code generation.
pub struct Config {
    pub out_dir: Option<PathBuf>,
}

impl Config {
    pub fn compile_abi(&self, abis: &[impl AsRef<Path>]) -> Result<()> {
        let target: PathBuf = self.out_dir.clone().map(Ok).unwrap_or_else(|| {
            env::var_os("OUT_DIR")
                .ok_or_else(|| anyhow!("OUT_DIR environment variable is not set"))
                .map(|val| Into::into(val))
        })?;

        for abi in abis {
            let abi_path = abi.as_ref();

            let abi_path_no_ext = abi_path.with_extension("");
            let abi_filename = abi_path_no_ext
                .file_name()
                .ok_or_else(|| anyhow!("{:?} is not a valid ABI path", &abi_path))?;
            let rust_path = target.join(abi_filename).with_extension("rs");
            let abi_content = fs::read_to_string(abi_path)?;
            let metadata = serde_json::from_str::<Metadata>(&abi_content)?;

            let mut token_stream = proc_macro2::TokenStream::new();
            let mut registry = HashMap::<u32, String>::new();
            for type_def in metadata.types {
                let schema_json = serde_json::to_string(&type_def.schema).unwrap();

                let schema = serde_json::from_str(&schema_json).unwrap_or_else(|err| {
                    panic!(
                        "Cannot parse `{}` as JSON: {}",
                        abi_path.to_string_lossy(),
                        err
                    )
                });

                let mut expander = Expander::new("", "", &schema);
                token_stream.extend(expander.expand(&schema));
                registry.insert(type_def.id, schema.title.clone().unwrap());
            }

            token_stream.extend(quote! {
                pub struct ExtContract {
                    pub contract: workspaces::Contract,
                }
            });

            let mut methods_stream = proc_macro2::TokenStream::new();
            for method in metadata.methods {
                let name = format_ident!("{}", method.name);
                let arg_names = method
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format_ident!("arg{}", i))
                    .collect::<Vec<_>>();
                let args = method
                    .args
                    .iter()
                    .zip(arg_names.iter())
                    .map(|(arg_type_id, arg_name)| {
                        let arg_type = format_ident!("{}", registry.get(arg_type_id).unwrap());
                        quote! { #arg_name: #arg_type }
                    })
                    .collect::<Vec<_>>();
                let return_type =
                    format_ident!("{}", registry.get(&method.result.unwrap()).unwrap());
                let name_str = name.to_string();
                if method.is_view {
                    methods_stream.extend(quote! {
                        pub async fn #name(
                            &self,
                            worker: &workspaces::Worker<impl workspaces::Network>,
                            #(#args),*
                        ) -> anyhow::Result<#return_type> {
                            let result = self.contract
                                .call(worker, #name_str)
                                .args_json([#(#arg_names),*])?
                                .view()
                                .await?;
                            result.json::<#return_type>()
                        }
                    });
                } else {
                    methods_stream.extend(quote! {
                        pub async fn #name(
                            &self,
                            worker: &workspaces::Worker<impl workspaces::Network>,
                            gas: near_primitives::types::Gas,
                            deposit: near_primitives::types::Balance,
                            #(#args),*
                        ) -> anyhow::Result<#return_type> {
                            let result = self.contract
                                .call(worker, #name_str)
                                .args_json([#(#arg_names),*])?
                                .gas(gas)
                                .deposit(deposit)
                                .transact()
                                .await?;
                            result.json::<#return_type>()
                        }
                    });
                }
            }

            token_stream.extend(quote! {
                impl ExtContract {
                    #methods_stream
                }
            });

            let mut rust_file = File::create(rust_path)?;
            write!(rust_file, "{}", token_stream.to_string())?;
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Config {
        Config { out_dir: None }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;
    use quote::quote;
    use std::fs;
    use tempdir::TempDir;

    #[test]
    fn test_compile_abi() -> anyhow::Result<()> {
        let tmp_dir = TempDir::new("adder-generated-code")?;
        let tmp_dir_path = tmp_dir.into_path();
        let config: Config = Config {
            out_dir: Some(tmp_dir_path.clone()),
        };

        config.compile_abi(&["tests/adder-metadata.json"])?;

        let generated_code = fs::read_to_string(tmp_dir_path.join("adder-metadata.rs"))?;
        let expected = quote! {
            pub type Pair = Vec<i64>;
            pub struct ExtContract {
                pub contract: workspaces::Contract,
            }
            impl ExtContract {
                pub async fn add(
                    &self,
                    worker: &workspaces::Worker<impl workspaces::Network>,
                    arg0: Pair,
                    arg1: Pair
                ) -> anyhow::Result<Pair> {
                    let result = self
                        .contract
                        .call(worker, "add")
                        .args_json([arg0, arg1])?
                        .view()
                        .await?;
                    result.json::<Pair>()
                }
            }
        };
        assert_eq!(expected.to_string(), generated_code);

        Ok(())
    }
}