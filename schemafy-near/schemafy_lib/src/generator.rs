use crate::Expander;
use near_sdk::Metadata;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

/// A configurable builder for generating Rust types from a JSON
/// schema.
///
/// The default options are usually fine. In that case, you can use
/// the [`generate()`](fn.generate.html) convenience method instead.
#[derive(Debug, PartialEq)]
#[must_use]
pub struct Generator<'a, 'b> {
    /// The name of the root type defined by the schema. If the schema
    /// does not define a root type (some schemas are simply a
    /// collection of definitions) then simply pass `None`.
    pub contract_name: String,
    /// The module path to this crate. Some generated code may make
    /// use of types defined in this crate. Unless you have
    /// re-exported this crate or imported it under a different name,
    /// the default should be fine.
    pub schemafy_path: &'a str,
    /// The JSON schema file to read
    pub input_file: &'b Path,
}

impl<'a, 'b> Generator<'a, 'b> {
    /// Get a builder for the Generator
    pub fn builder() -> GeneratorBuilder<'a, 'b> {
        GeneratorBuilder::default()
    }

    pub fn generate(&self) -> proc_macro2::TokenStream {
        let input_file = if self.input_file.is_relative() {
            let crate_root = get_crate_root().unwrap();
            crate_root.join(self.input_file)
        } else {
            PathBuf::from(self.input_file)
        };

        let metadata_json = std::fs::read_to_string(&input_file).unwrap_or_else(|err| {
            panic!("Unable to read `{}`: {}", input_file.to_string_lossy(), err)
        });

        let near_metadata = serde_json::from_str::<Metadata>(&metadata_json).expect("123");

        let mut token_stream = proc_macro2::TokenStream::new();
        let mut registry = HashMap::<u32, String>::new();
        for t in near_metadata.types {
            let schema_json = serde_json::to_string(&t.schema).unwrap();

            let schema = serde_json::from_str(&schema_json).unwrap_or_else(|err| {
                panic!("Cannot parse `{}` as JSON: {}", input_file.to_string_lossy(), err)
            });

            let mut expander = Expander::new(&self.contract_name, self.schemafy_path, &schema);
            token_stream.extend(expander.expand(&schema));
            registry.insert(t.id, schema.title.clone().unwrap());
        }

        let methods = near_metadata
            .methods
            .iter()
            .map(|m| {
                let name = format_ident!("{}", m.name);
                let result_type = m
                    .result
                    .map(|r_id| {
                        let r_type = format_ident!(
                            "{}",
                            registry.get(&r_id).expect("Unexpected result type")
                        );
                        quote! { -> #r_type }
                    })
                    .unwrap_or_else(|| quote! {});
                let args = m
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, a_id)| {
                        let a_type = format_ident!(
                            "{}",
                            registry.get(&a_id).expect("Unexpected argument type")
                        );
                        let a_name = format_ident!("arg{}", &i);
                        quote! { #a_name: #a_type }
                    })
                    .collect::<Vec<_>>();
                quote! { fn #name(&self, #(#args),*) #result_type; }
            })
            .collect::<Vec<_>>();

        let ext_contract_ident = format_ident!("{}", &self.contract_name);
        token_stream.extend(quote! {
            #[near_sdk::ext_contract]
            pub trait #ext_contract_ident {
                #(#methods)*
            }
        });

        token_stream
    }

    pub fn generate_to_file<P: ?Sized + AsRef<Path>>(&self, output_file: &'b P) -> io::Result<()> {
        use std::process::Command;
        let tokens = self.generate();
        let out = tokens.to_string();
        std::fs::write(output_file, &out)?;
        Command::new("rustfmt").arg(output_file.as_ref().as_os_str()).output()?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
#[must_use]
pub struct GeneratorBuilder<'a, 'b> {
    inner: Generator<'a, 'b>,
}

impl<'a, 'b> Default for GeneratorBuilder<'a, 'b> {
    fn default() -> Self {
        Self {
            inner: Generator {
                contract_name: "".to_string(),
                schemafy_path: "::schemafy_core::",
                input_file: Path::new("schema.json"),
            },
        }
    }
}

impl<'a, 'b> GeneratorBuilder<'a, 'b> {
    pub fn with_contract_name(mut self, contract_name: String) -> Self {
        self.inner.contract_name = contract_name;
        self
    }
    pub fn with_contract_name_str(mut self, contract_name: &str) -> Self {
        self.inner.contract_name = contract_name.to_string();
        self
    }
    pub fn with_input_file<P: ?Sized + AsRef<Path>>(mut self, input_file: &'b P) -> Self {
        self.inner.input_file = input_file.as_ref();
        self
    }
    pub fn with_schemafy_path(mut self, schemafy_path: &'a str) -> Self {
        self.inner.schemafy_path = schemafy_path;
        self
    }
    pub fn build(self) -> Generator<'a, 'b> {
        self.inner
    }
}

fn get_crate_root() -> std::io::Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_MANIFEST_DIR") {
        return Ok(PathBuf::from(path));
    }

    let current_dir = std::env::current_dir()?;

    for p in current_dir.ancestors() {
        if std::fs::read_dir(p)?
            .into_iter()
            .filter_map(Result::ok)
            .any(|p| p.file_name().eq("Cargo.toml"))
        {
            return Ok(PathBuf::from(p));
        }
    }

    Ok(current_dir)
}