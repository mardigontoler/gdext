/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use crate::derive_godot_class::make_existence_check;
use crate::util::{ident, KvParser};
use crate::ParseResult;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

use super::Fields;

pub struct FieldExport {
    getter: GetterSetter,
    setter: GetterSetter,
    hint: Option<ExportHint>,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum GetterSetter {
    /// Getter/setter should be omitted, field is write/read only.
    Omitted,

    /// Trivial getter/setter should be autogenerated.
    Generated,

    /// Getter/setter is hand-written by the user, and here is its identifier.
    Custom(Ident),
}

impl GetterSetter {
    fn parse(parser: &mut KvParser, key: &str) -> ParseResult<Self> {
        Ok(match parser.handle_any(key) {
            // No `get` argument
            None => GetterSetter::Omitted,
            Some(value) => match value {
                // `get` without value
                None => GetterSetter::Generated,
                // `get = expr`
                Some(value) => GetterSetter::Custom(value.ident()?),
            },
        })
    }
}

#[derive(Clone)]
pub struct ExportHint {
    hint_type: Ident,
    description: TokenStream,
}

impl FieldExport {
    pub(crate) fn new_from_kv(parser: &mut KvParser) -> ParseResult<FieldExport> {
        let mut getter = GetterSetter::parse(parser, "get")?;
        let mut setter = GetterSetter::parse(parser, "set")?;
        if getter == GetterSetter::Omitted && setter == GetterSetter::Omitted {
            getter = GetterSetter::Generated;
            setter = GetterSetter::Generated;
        }

        let hint = parser
            .handle_ident("hint")?
            .map(|hint_type| {
                Ok(ExportHint {
                    hint_type,
                    description: parser.handle_expr_required("hint_desc")?,
                })
            })
            .transpose()?;

        Ok(FieldExport {
            getter,
            setter,
            hint,
        })
    }
}

pub(super) fn make_exports_impl(class_name: &Ident, fields: &Fields) -> TokenStream {
    let mut getter_setter_impls = Vec::new();
    let mut export_tokens = Vec::new();

    for field in &fields.all_fields {
        let Some(export) = &field.export else { continue; };
        let field_name = field.name.to_string();
        let field_ident = ident(&field_name);
        let field_type = field.ty.clone();

        let export_info = quote! {
            let mut export_info = <#field_type as ::godot::bind::property::Export>::default_export_info();
        };

        let custom_hint = if let Some(ExportHint {
            hint_type,
            description,
        }) = export.hint.clone()
        {
            quote! {
                export_info.hint = ::godot::engine::global::PropertyHint::#hint_type;
                export_info.hint_string = ::godot::builtin::GodotString::from(#description);
            }
        } else {
            quote! {}
        };

        let getter_name;
        match &export.getter {
            GetterSetter::Omitted => {
                getter_name = "".to_owned();
            }
            GetterSetter::Generated => {
                getter_name = format!("get_{field_name}");
                let getter_ident = ident(&getter_name);
                let signature = quote! {
                    fn #getter_ident(&self) -> #field_type
                };
                getter_setter_impls.push(quote! {
                    pub #signature {
                        ::godot::bind::property::Export::export(&self.#field_ident)
                    }
                });
                export_tokens.push(quote! {
                    ::godot::private::gdext_register_method!(#class_name, #signature);
                });
            }
            GetterSetter::Custom(getter_ident) => {
                getter_name = getter_ident.to_string();
                export_tokens.push(make_existence_check(getter_ident));
            }
        }

        let setter_name;
        match &export.setter {
            GetterSetter::Omitted => {
                setter_name = "".to_owned();
            }
            GetterSetter::Generated => {
                setter_name = format!("set_{field_name}");
                let setter_ident = ident(&setter_name);
                let signature = quote! {
                    fn #setter_ident(&mut self, #field_ident: #field_type)
                };
                getter_setter_impls.push(quote! {
                    pub #signature {
                        self.#field_ident = #field_ident;
                    }
                });
                export_tokens.push(quote! {
                    ::godot::private::gdext_register_method!(#class_name, #signature);
                });
            }
            GetterSetter::Custom(setter_ident) => {
                setter_name = setter_ident.to_string();
                export_tokens.push(make_existence_check(setter_ident));
            }
        };

        export_tokens.push(quote! {
            use ::godot::builtin::meta::VariantMetadata;

            let class_name = ::godot::builtin::StringName::from(#class_name::CLASS_NAME);

            #export_info

            #custom_hint

            let property_info = export_info.to_property_info::<#class_name>(
                #field_name.into(),
                ::godot::engine::global::PropertyUsageFlags::PROPERTY_USAGE_DEFAULT
            );
            let property_info_sys = property_info.property_sys();

            let getter_name = ::godot::builtin::StringName::from(#getter_name);
            let setter_name = ::godot::builtin::StringName::from(#setter_name);
            unsafe {
                ::godot::sys::interface_fn!(classdb_register_extension_class_property)(
                    ::godot::sys::get_library(),
                    class_name.string_sys(),
                    std::ptr::addr_of!(property_info_sys),
                    setter_name.string_sys(),
                    getter_name.string_sys(),
                );
            }
        });
    }

    let enforce_godot_api_impl = if !export_tokens.is_empty() {
        quote! {
            const MUST_HAVE_GODOT_API_IMPL: () = <#class_name as ::godot::private::Cannot_export_without_godot_api_impl>::EXISTS;
        }
    } else {
        TokenStream::new()
    };

    quote! {
        impl #class_name {
            #enforce_godot_api_impl

            #(#getter_setter_impls)*
        }

        impl ::godot::obj::cap::ImplementsGodotExports for #class_name {
            fn __register_exports() {
                #(
                    {
                        #export_tokens
                    }
                )*
            }
        }
    }
}
