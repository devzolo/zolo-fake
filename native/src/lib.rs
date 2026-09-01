//! High-throughput native executor for the `fake` Zolo package.
//!
//! A `NativeFaker` owns its cursor and uniqueness sets. Generation happens a
//! column at a time and crosses the plugin boundary once per chunk, instead of
//! once per value. Every leaf gets an independent seed derived from
//! `(seed, row, field path, retry)`, so schema field reordering cannot perturb
//! unrelated values.

use std::collections::{HashMap, HashSet};

use fake::faker::address::raw::{CityName, PostCode, StateName, StreetName};
use fake::faker::company::raw::{CompanyName, Industry, Profession};
use fake::faker::internet::raw::{DomainSuffix, IPv4, SafeEmail, Username};
use fake::faker::lorem::raw::{Paragraph, Sentence, Word};
use fake::faker::name::raw::{FirstName, LastName, Name};
use fake::locales::{EN, FR_FR, PT_BR};
use fake::rand::rngs::StdRng;
use fake::rand::SeedableRng;
use fake::{Fake as _, Faker as RawFaker};
use zolo_plugin::handle::{get_handle, register_handle_tagged, remove_handle, PluginHandle};
use zolo_plugin::prelude::*;

type PV = PluginValue;

#[derive(Debug)]
struct NativeFakerData {
    seed: u64,
    locale: String,
    fallback_locale: String,
    cursor: i64,
    chunk_size: usize,
    retry_limit: usize,
    unique: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone)]
struct FieldPlan {
    name: String,
    path: String,
    kind: String,
    inner_kind: String,
    provider: String,
    min: i64,
    max: i64,
    min_len: usize,
    max_len: usize,
    probability: f64,
    optional: bool,
    ignored: bool,
    unique: bool,
    variants: Vec<String>,
}

fn sig(params: &[(&str, TypeHint)], returns: TypeHint) -> FnSignature {
    FnSignature {
        params: params
            .iter()
            .map(|(name, type_hint)| ParamDef {
                name: (*name).to_string(),
                type_hint: type_hint.clone(),
            })
            .collect(),
        returns,
    }
}

fn arg<'a>(args: &'a [PV], index: usize, name: &str) -> Result<&'a PV, PluginError> {
    args.get(index)
        .ok_or_else(|| PluginError::Custom(format!("fake-native: missing argument `{name}`")))
}

fn require_handle(args: &[PV]) -> Result<PluginHandle<NativeFakerData>, PluginError> {
    let id = arg(args, 0, "self")?.as_handle_id()?;
    get_handle::<NativeFakerData>(id)
        .ok_or_else(|| PluginError::Custom("fake-native: NativeFaker handle not found".into()))
}

fn table_get<'a>(value: &'a PV, key: &str) -> Option<&'a PV> {
    let PV::Table(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find_map(|(entry_key, value)| match entry_key {
            PV::String(entry_key) if entry_key == key => Some(value),
            _ => None,
        })
}

fn value_string(value: Option<&PV>, default: &str) -> String {
    match value {
        Some(PV::String(value)) => value.clone(),
        _ => default.to_string(),
    }
}

fn value_i64(value: Option<&PV>, default: i64) -> i64 {
    match value {
        Some(PV::Integer(value)) => *value,
        Some(PV::Number(value)) => *value as i64,
        _ => default,
    }
}

fn value_f64(value: Option<&PV>, default: f64) -> f64 {
    match value {
        Some(PV::Number(value)) => *value,
        Some(PV::Integer(value)) => *value as f64,
        _ => default,
    }
}

fn value_bool(value: Option<&PV>, default: bool) -> bool {
    match value {
        Some(PV::Bool(value)) => *value,
        _ => default,
    }
}

fn value_strings(value: Option<&PV>) -> Vec<String> {
    let Some(PV::Table(entries)) = value else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(_, value)| match value {
            PV::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn parse_field(value: &PV) -> Result<FieldPlan, PluginError> {
    let name = value_string(table_get(value, "name"), "");
    if name.is_empty() {
        return Err(PluginError::Custom(
            "fake-native: field plan is missing `name`".into(),
        ));
    }
    let min_len = value_i64(table_get(value, "min_len"), 0).max(0) as usize;
    let max_len = value_i64(table_get(value, "max_len"), 4).max(0) as usize;
    if min_len > max_len {
        return Err(PluginError::Custom(format!(
            "fake-native: invalid length range for `{name}`"
        )));
    }
    Ok(FieldPlan {
        path: value_string(table_get(value, "path"), &name),
        kind: value_string(table_get(value, "kind"), "Any"),
        inner_kind: value_string(table_get(value, "inner_kind"), ""),
        provider: value_string(table_get(value, "provider"), "Unsupported"),
        min: value_i64(table_get(value, "min"), 0),
        max: value_i64(table_get(value, "max"), 1000),
        min_len,
        max_len,
        probability: value_f64(table_get(value, "probability"), 0.5),
        optional: value_bool(table_get(value, "optional"), false),
        ignored: value_bool(table_get(value, "ignored"), false),
        unique: value_bool(table_get(value, "unique"), false),
        variants: value_strings(table_get(value, "variants")),
        name,
    })
}

fn parse_fields(plan: &PV) -> Result<Vec<FieldPlan>, PluginError> {
    let Some(PV::Table(fields)) = table_get(plan, "fields") else {
        return Err(PluginError::Custom(
            "fake-native: plan is missing the `fields` array".into(),
        ));
    };
    fields.iter().map(|(_, field)| parse_field(field)).collect()
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn sub_seed(seed: u64, row: i64, path: &str, retry: usize) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash ^= row as u64;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    hash ^= retry as u64;
    splitmix64(hash)
}

fn normalized_locale(locale: &str, fallback: &str) -> Result<String, PluginError> {
    let normalized = locale.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "en" | "en-us" => Ok("en".into()),
        "pt" | "pt-br" => Ok("pt-BR".into()),
        "fr" | "fr-fr" => Ok("fr-FR".into()),
        _ if !fallback.trim().is_empty() && locale != fallback => normalized_locale(fallback, ""),
        _ => Err(PluginError::Custom(format!(
            "fake-native: unsupported locale `{locale}`; use en, pt-BR, fr-FR, or pass an explicit fallback_locale"
        ))),
    }
}

macro_rules! localized_string {
    ($locale:expr, $rng:expr, $en:expr, $pt:expr, $fr:expr) => {{
        match $locale {
            "pt-BR" => ($pt).fake_with_rng::<String, _>($rng),
            "fr-FR" => ($fr).fake_with_rng::<String, _>($rng),
            _ => ($en).fake_with_rng::<String, _>($rng),
        }
    }};
}

fn localized_provider(provider: &str, locale: &str, rng: &mut StdRng) -> Option<String> {
    Some(match provider {
        "Word" => localized_string!(locale, rng, Word(EN), Word(PT_BR), Word(FR_FR)),
        "FirstName" => {
            localized_string!(
                locale,
                rng,
                FirstName(EN),
                FirstName(PT_BR),
                FirstName(FR_FR)
            )
        }
        "LastName" => {
            localized_string!(locale, rng, LastName(EN), LastName(PT_BR), LastName(FR_FR))
        }
        "FullName" => localized_string!(locale, rng, Name(EN), Name(PT_BR), Name(FR_FR)),
        "Username" => {
            localized_string!(locale, rng, Username(EN), Username(PT_BR), Username(FR_FR))
        }
        "SafeEmail" => {
            localized_string!(
                locale,
                rng,
                SafeEmail(EN),
                SafeEmail(PT_BR),
                SafeEmail(FR_FR)
            )
        }
        "Domain" => {
            let user =
                localized_string!(locale, rng, Username(EN), Username(PT_BR), Username(FR_FR));
            let suffix = localized_string!(
                locale,
                rng,
                DomainSuffix(EN),
                DomainSuffix(PT_BR),
                DomainSuffix(FR_FR)
            );
            format!("{}.{}", user.to_ascii_lowercase(), suffix)
        }
        "Ipv4" => localized_string!(locale, rng, IPv4(EN), IPv4(PT_BR), IPv4(FR_FR)),
        "City" => {
            localized_string!(locale, rng, CityName(EN), CityName(PT_BR), CityName(FR_FR))
        }
        "State" => {
            localized_string!(
                locale,
                rng,
                StateName(EN),
                StateName(PT_BR),
                StateName(FR_FR)
            )
        }
        "Postcode" => {
            localized_string!(locale, rng, PostCode(EN), PostCode(PT_BR), PostCode(FR_FR))
        }
        "Street" => {
            localized_string!(
                locale,
                rng,
                StreetName(EN),
                StreetName(PT_BR),
                StreetName(FR_FR)
            )
        }
        "CompanyName" => localized_string!(
            locale,
            rng,
            CompanyName(EN),
            CompanyName(PT_BR),
            CompanyName(FR_FR)
        ),
        "Profession" => localized_string!(
            locale,
            rng,
            Profession(EN),
            Profession(PT_BR),
            Profession(FR_FR)
        ),
        "Industry" => {
            localized_string!(locale, rng, Industry(EN), Industry(PT_BR), Industry(FR_FR))
        }
        "Sentence" => localized_string!(
            locale,
            rng,
            Sentence(EN, 8..15),
            Sentence(PT_BR, 8..15),
            Sentence(FR_FR, 8..15)
        ),
        "Paragraph" => localized_string!(
            locale,
            rng,
            Paragraph(EN, 3..6),
            Paragraph(PT_BR, 3..6),
            Paragraph(FR_FR, 3..6)
        ),
        _ => return None,
    })
}

fn table_array(values: Vec<PV>) -> PV {
    PV::Table(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (PV::Integer(index as i64 + 1), value))
            .collect(),
    )
}

fn fake_uuid_v4(rng: &mut StdRng) -> String {
    let high = RawFaker.fake_with_rng::<u64, _>(rng);
    let low = RawFaker.fake_with_rng::<u64, _>(rng);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

fn generate_value(
    seed: u64,
    locale: &str,
    field: &FieldPlan,
    row: i64,
    retry: usize,
) -> Result<PV, PluginError> {
    if field.ignored {
        return Ok(PV::Nil);
    }
    let mut rng = StdRng::seed_from_u64(sub_seed(seed, row, &field.path, retry));
    if field.optional {
        let threshold = (field.probability.clamp(0.0, 1.0) * 1_000_000.0) as u32;
        let sample = (0u32..1_000_000u32).fake_with_rng::<u32, _>(&mut rng);
        if sample >= threshold {
            return Ok(PV::Nil);
        }
    }

    if field.kind == "list" {
        let length = if field.min_len == field.max_len {
            field.min_len
        } else {
            (field.min_len..=field.max_len).fake_with_rng::<usize, _>(&mut rng)
        };
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(match field.inner_kind.as_str() {
                "int" => PV::Integer((field.min..=field.max).fake_with_rng::<i64, _>(&mut rng)),
                "str" => PV::String(
                    localized_provider("Word", locale, &mut rng)
                        .expect("Word provider is always available"),
                ),
                other => {
                    return Err(PluginError::Custom(format!(
                        "fake-native: list element type `{other}` is not supported for `{}`",
                        field.name
                    )));
                }
            });
        }
        return Ok(table_array(values));
    }

    let value = match field.provider.as_str() {
        "Int" => PV::Integer((field.min..=field.max).fake_with_rng::<i64, _>(&mut rng)),
        "Float" => {
            let min = field.min as f64;
            let max = field.max as f64;
            PV::Number((min..=max).fake_with_rng::<f64, _>(&mut rng))
        }
        "Chance" => {
            let threshold = (field.probability.clamp(0.0, 1.0) * 1_000_000.0) as u32;
            let sample = (0u32..1_000_000u32).fake_with_rng::<u32, _>(&mut rng);
            PV::Bool(sample < threshold)
        }
        "UuidV4" => PV::String(fake_uuid_v4(&mut rng)),
        "Sequence" => PV::String(row.to_string()),
        "Enum" => {
            if field.variants.is_empty() {
                return Err(PluginError::Custom(format!(
                    "fake-native: enum `{}` has no variants",
                    field.name
                )));
            }
            let index = (0..field.variants.len()).fake_with_rng::<usize, _>(&mut rng);
            PV::String(field.variants[index].clone())
        }
        "Nominal" => {
            return Err(PluginError::Custom(format!(
                "fake-native: nested nominal field `{}` uses the portable executor in v1",
                field.name
            )));
        }
        provider => match localized_provider(provider, locale, &mut rng) {
            Some(value) => PV::String(value),
            None => {
                return Err(PluginError::Custom(format!(
                    "fake-native: unsupported provider `{provider}` for `{}`",
                    field.name
                )));
            }
        },
    };
    Ok(value)
}

fn unique_key(value: &PV) -> String {
    match value {
        PV::Nil => "nil".into(),
        PV::Bool(value) => format!("b:{value}"),
        PV::Integer(value) => format!("i:{value}"),
        PV::Number(value) => format!("f:{:016x}", value.to_bits()),
        PV::String(value) => format!("s:{value}"),
        other => format!("v:{other:?}"),
    }
}

fn disambiguate_unique(value: PV, provider: &str, row: i64, retry: usize) -> PV {
    if retry == 0 || provider != "SafeEmail" {
        return value;
    }
    let PV::String(email) = value else {
        return value;
    };
    let Some((local, domain)) = email.rsplit_once('@') else {
        return PV::String(email);
    };
    // fake-rs' localized name pools are intentionally finite. Preserve the
    // email shape while deriving a deterministic, row-scoped local suffix so
    // `unique: true` remains useful for database-scale fixture batches.
    PV::String(format!("{local}+{row}-{retry}@{domain}"))
}

fn native_new(args: &[PV]) -> Result<PV, PluginError> {
    let seed = arg(args, 0, "seed")?.as_i64()? as u64;
    let locale = arg(args, 1, "locale")?.as_string()?.to_string();
    let fallback_locale = arg(args, 2, "fallback_locale")?.as_string()?.to_string();
    let chunk_size = arg(args, 3, "chunk_size")?.as_i64()?;
    let retry_limit = arg(args, 4, "retry_limit")?.as_i64()?;
    if chunk_size <= 0 {
        return Err(PluginError::Custom(
            "fake-native: chunk_size must be positive".into(),
        ));
    }
    if retry_limit <= 0 {
        return Err(PluginError::Custom(
            "fake-native: retry_limit must be positive".into(),
        ));
    }
    let data = NativeFakerData {
        seed,
        locale: normalized_locale(&locale, &fallback_locale)?,
        fallback_locale,
        cursor: 0,
        chunk_size: chunk_size as usize,
        retry_limit: retry_limit as usize,
        unique: HashMap::new(),
    };
    let handle = PluginHandle::new(data, "NativeFaker");
    Ok(register_handle_tagged(&handle))
}

fn native_reserve(args: &[PV]) -> Result<PV, PluginError> {
    let handle = require_handle(args)?;
    let count = arg(args, 1, "count")?.as_i64()?;
    if count < 0 {
        return Err(PluginError::Custom(
            "fake-native: count must be non-negative".into(),
        ));
    }
    let mut data = handle.get_mut()?;
    let start = data.cursor;
    data.cursor = data.cursor.saturating_add(count);
    Ok(PV::Integer(start))
}

fn native_chunk_size(args: &[PV]) -> Result<PV, PluginError> {
    let handle = require_handle(args)?;
    Ok(PV::Integer(handle.get()?.chunk_size as i64))
}

fn native_engine(args: &[PV]) -> Result<PV, PluginError> {
    let handle = require_handle(args)?;
    let data = handle.get()?;
    Ok(PV::Table(vec![
        (PV::String("name".into()), PV::String("fake-rs".into())),
        (PV::String("version".into()), PV::String("5.1".into())),
        (PV::String("locale".into()), PV::String(data.locale.clone())),
        (
            PV::String("fallback_locale".into()),
            PV::String(data.fallback_locale.clone()),
        ),
    ]))
}

fn native_batch(args: &[PV]) -> Result<PV, PluginError> {
    let handle = require_handle(args)?;
    let plan = arg(args, 1, "plan")?;
    let start = arg(args, 2, "start")?.as_i64()?;
    let count = arg(args, 3, "count")?.as_i64()?;
    if start < 0 || count < 0 {
        return Err(PluginError::Custom(
            "fake-native: start and count must be non-negative".into(),
        ));
    }
    let fields = parse_fields(plan)?;
    let schema_hash = value_i64(table_get(plan, "schema_hash"), 0);
    let mut data = handle.get_mut()?;
    let seed = data.seed;
    let locale = data.locale.clone();
    let retry_limit = data.retry_limit;
    let mut columns = Vec::with_capacity(fields.len());

    for field in fields {
        let unique_namespace = format!("{schema_hash}:{}", field.path);
        let mut values = Vec::with_capacity(count as usize);
        for offset in 0..count {
            let row = start + offset;
            let mut accepted = None;
            for retry in 0..retry_limit {
                let value = generate_value(seed, &locale, &field, row, retry)?;
                let value = disambiguate_unique(value, &field.provider, row, retry);
                if !field.unique {
                    accepted = Some(value);
                    break;
                }
                let key = unique_key(&value);
                let seen = data.unique.entry(unique_namespace.clone()).or_default();
                if seen.insert(key) {
                    accepted = Some(value);
                    break;
                }
            }
            values.push(accepted.ok_or_else(|| {
                PluginError::Custom(format!(
                    "fake-native: unique generation for `{}` exhausted {} retries at row {row}",
                    field.name, retry_limit
                ))
            })?);
        }
        columns.push((PV::String(field.name), table_array(values)));
    }
    Ok(PV::Table(columns))
}

#[derive(Default)]
pub struct FakeNativePlugin;

impl ZoloPlugin for FakeNativePlugin {
    fn name(&self) -> &str {
        "fake_native"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn register(&self, ctx: &mut PluginContext) {
        let handle = TypeHint::Handle("NativeFaker".into());
        ctx.add_class(ClassDef {
            name: "NativeFaker".into(),
            constructor: Some(native_new),
            constructor_sig: Some(sig(
                &[
                    ("seed", TypeHint::Integer),
                    ("locale", TypeHint::String),
                    ("fallback_locale", TypeHint::String),
                    ("chunk_size", TypeHint::Integer),
                    ("retry_limit", TypeHint::Integer),
                ],
                handle.clone(),
            )),
            methods: vec![
                (
                    "reserve".into(),
                    native_reserve as NativePluginFn,
                    sig(
                        &[("self", handle.clone()), ("count", TypeHint::Integer)],
                        TypeHint::Integer,
                    ),
                ),
                (
                    "chunk_size".into(),
                    native_chunk_size as NativePluginFn,
                    sig(&[("self", handle.clone())], TypeHint::Integer),
                ),
                (
                    "engine".into(),
                    native_engine as NativePluginFn,
                    sig(&[("self", handle.clone())], TypeHint::Table),
                ),
                (
                    "batch".into(),
                    native_batch as NativePluginFn,
                    sig(
                        &[
                            ("self", handle.clone()),
                            ("plan", TypeHint::Table),
                            ("start", TypeHint::Integer),
                            ("count", TypeHint::Integer),
                        ],
                        TypeHint::Table,
                    ),
                ),
            ],
            static_methods: vec![],
            finalizer: Some(|id| {
                let _ = remove_handle(id);
            }),
        });
    }
}

zolo_plugin::export_plugin!(FakeNativePlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substreams_are_stable_and_path_sensitive() {
        assert_eq!(
            sub_seed(42, 7, "User.name", 0),
            sub_seed(42, 7, "User.name", 0)
        );
        assert_ne!(
            sub_seed(42, 7, "User.name", 0),
            sub_seed(42, 7, "User.email", 0)
        );
        assert_ne!(
            sub_seed(42, 7, "User.name", 0),
            sub_seed(42, 8, "User.name", 0)
        );
    }

    #[test]
    fn locale_fallback_is_explicit() {
        assert_eq!(normalized_locale("pt_BR", "").unwrap(), "pt-BR");
        assert_eq!(normalized_locale("unknown", "fr-FR").unwrap(), "fr-FR");
        assert!(normalized_locale("unknown", "").is_err());
    }

    #[test]
    fn unique_email_retry_preserves_email_shape_and_uses_row() {
        let value = disambiguate_unique(PV::String("ana@example.com".into()), "SafeEmail", 42, 1);
        assert!(matches!(
            value,
            PV::String(ref email) if email == "ana+42-1@example.com"
        ));
    }
}
