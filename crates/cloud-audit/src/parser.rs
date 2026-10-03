//! Pure parsers over unwrapped `batchexecute` payloads. Every access is
//! tolerant: a missing or mistyped field becomes `None`; only a payload whose
//! top-level shape is wrong is an error. Indices mirror Google-Photos-Toolkit
//! `src/api/parser.ts` and gpwc `gpwc/parser.py`.

use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};

/// One `swbisb` remote match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HashLookup {
    pub hash_b64: String,
    pub media_key: String,
    pub dedup_key: Option<String>,
    /// Camera make/model from the item's EXIF (`cameraInfo`), e.g. "iPhone 17
    /// Pro" for a file pushed to a Pixel. Not the uploading device; only the
    /// quota flags say whether the Pixel perk applied. Verified live 2026-10-03.
    pub device_model: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub timestamp_ms: Option<i64>,
    pub creation_timestamp_ms: Option<i64>,
}

/// Quota-relevant item fields from `EWgK9e`, `VrseUb` or `fDcn4b`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ItemInfo {
    pub media_key: String,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub takes_up_space: Option<bool>,
    pub space_taken: Option<u64>,
    pub is_original_quality: Option<bool>,
}

fn at<'a>(value: &'a Value, path: &[usize]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, &i| v.as_array()?.get(i))
}
fn last(value: &Value) -> Option<&Value> {
    value.as_array()?.last()
}
fn string(value: Option<&Value>) -> Option<String> {
    value?.as_str().map(str::to_owned)
}
fn non_empty(value: Option<&Value>) -> Option<String> {
    string(value).filter(|s| !s.is_empty())
}
fn uint(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}
fn int(value: Option<&Value>) -> Option<i64> {
    value?.as_i64()
}
/// Optional array: absent/null is `None`, any other non-array is an error.
fn list<'a>(value: Option<&'a Value>, what: &str) -> Result<Option<&'a Vec<Value>>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => Ok(Some(items)),
        Some(_) => Err(Error::Parse(format!("{what} is not an array"))),
    }
}

/// Quota vector `[takesUpSpace, spaceTaken, quality, ...]`; e.g. `[2,0,2,0,null,1]`.
/// Toolkit: takesUpSpace = `v[0] === 1`, isOriginalQuality = `v[2] === 2`. A
/// non-numeric entry is `None` here (Toolkit would report `false` for null).
fn quota(vector: Option<&Value>) -> (Option<bool>, Option<u64>, Option<bool>) {
    let field = |i: usize| vector.and_then(|v| at(v, &[i])).and_then(Value::as_u64);
    (field(0).map(|v| v == 1), field(1), field(2).map(|v| v == 2))
}

/// `swbisb`: `data[0]` lists `[hash_b64, item]` for hashes that matched;
/// unmatched hashes are absent. Per item (Toolkit `remoteMatchParse`):
/// media key `[1][0]`, `[1][1]` = `[thumb, width, height, .., cameraInfo@8]`,
/// timestamp `[1][2]`, dedup key `[1][3]`, tz offset `[1][4]`, creation
/// timestamp `[1][5]`. cameraInfo is `[w, h, _, _, [make, model, ..]]`.
pub fn parse_hash_matches(data: &Value) -> Result<Vec<HashLookup>> {
    if !data.is_array() {
        return Err(Error::Parse("swbisb payload is not an array".into()));
    }
    let Some(items) = list(at(data, &[0]), "swbisb data[0]")? else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let entry = at(item, &[1])?;
            Some(HashLookup {
                hash_b64: non_empty(at(item, &[0]))?,
                media_key: non_empty(at(entry, &[0]))?,
                dedup_key: string(at(entry, &[3])),
                device_model: non_empty(at(entry, &[1, 8, 4, 1])),
                width: uint(at(entry, &[1, 1])),
                height: uint(at(entry, &[1, 2])),
                timestamp_ms: int(at(entry, &[2])),
                creation_timestamp_ms: int(at(entry, &[5])),
            })
        })
        .collect())
}

/// `EWgK9e`: the item list sits at `response[0][1]` (Toolkit
/// `getBatchMediaInfo`); see [`parse_batch_items`].
pub fn parse_batch_info(response: &Value) -> Result<Vec<ItemInfo>> {
    if !response.is_array() {
        return Err(Error::Parse("EWgK9e payload is not an array".into()));
    }
    match list(at(response, &[0, 1]), "EWgK9e response[0][1]")? {
        Some(_) => parse_batch_items(&response[0][1]),
        None => Ok(Vec::new()),
    }
}

/// `EWgK9e` item list (Toolkit `itemBulkMediaInfoParse`, gpwc
/// `ItemInfoBatch`): media key `[0]`, file name `[1][3]`, size `[1][9]`, quota
/// vector is the last element of `[1]`.
pub fn parse_batch_items(items: &Value) -> Result<Vec<ItemInfo>> {
    let items = items
        .as_array()
        .ok_or_else(|| Error::Parse("EWgK9e item list is not an array".into()))?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let detail = at(item, &[1]);
            let (takes_up_space, space_taken, is_original_quality) =
                quota(detail.and_then(last).filter(|v| v.is_array()));
            Some(ItemInfo {
                media_key: non_empty(at(item, &[0]))?,
                file_name: non_empty(detail.and_then(|d| at(d, &[3]))),
                size: uint(detail.and_then(|d| at(d, &[9]))),
                takes_up_space,
                space_taken,
                is_original_quality,
            })
        })
        .collect())
}

/// `VrseUb` (single item, lower confidence): media key `[0][0]`. The quota
/// vector is `ext["318563170"][0]`, where `ext` is the first object inside
/// `[0]` carrying a known extension key; Toolkit searches for it because its
/// index varies (gpwc assumes `[0][15]`). This RPC returns no file name or
/// size.
pub fn parse_item_info(data: &Value) -> Result<ItemInfo> {
    const KNOWN: [&str; 7] = [
        "15",
        "76647426",
        "146008172",
        "163238866",
        "225032867",
        "318563170",
        "525000000",
    ];
    let item = at(data, &[0])
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Parse("VrseUb data[0] is not an array".into()))?;
    let ext: Option<&Map<String, Value>> = item
        .iter()
        .filter_map(Value::as_object)
        .find(|o| o.keys().any(|k| KNOWN.contains(&k.as_str())));
    let (takes_up_space, space_taken, is_original_quality) = quota(
        ext.and_then(|o| o.get("318563170"))
            .and_then(|v| at(v, &[0])),
    );
    Ok(ItemInfo {
        media_key: non_empty(item.first())
            .ok_or_else(|| Error::Parse("VrseUb media key missing".into()))?,
        file_name: None,
        size: None,
        takes_up_space,
        space_taken,
        is_original_quality,
    })
}

/// `fDcn4b` (extended item info, Toolkit `itemInfoExtParse`): media key
/// `[0][0]`, file name `[0][2]`, size `[0][5]`, quota vector `[0][30]`.
pub fn parse_item_info_ext(data: &Value) -> Result<ItemInfo> {
    let item = at(data, &[0])
        .filter(|v| v.is_array())
        .ok_or_else(|| Error::Parse("fDcn4b data[0] is not an array".into()))?;
    let (takes_up_space, space_taken, is_original_quality) = quota(at(item, &[30]));
    Ok(ItemInfo {
        media_key: non_empty(at(item, &[0]))
            .ok_or_else(|| Error::Parse("fDcn4b media key missing".into()))?,
        file_name: non_empty(at(item, &[2])),
        size: uint(at(item, &[5])),
        takes_up_space,
        space_taken,
        is_original_quality,
    })
}
