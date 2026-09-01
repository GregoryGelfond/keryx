//! Doc comments from `SourceCodeInfo` (§13.1, §20): each element's leading comment,
//! keyed by its source-info path. Reading source info off the typed
//! `FileDescriptorProto` touches no options, so it does not bear on the §20
//! dynamic-option rule — options and source info are independent descriptor
//! regions. Source info is a precondition, not a guarantee (§13.1): absent → no
//! doc rides, still valid.

use prost_reflect::FileDescriptor;

/// The trimmed leading comment for the element at `path` in `file`, or `None` when
/// the set carried no source info for it. Location paths are matched exactly against
/// `SourceCodeInfo.location[*].path`.
pub(super) fn for_path(file: &FileDescriptor, path: &[i32]) -> Option<String> {
    let source = file.file_descriptor_proto().source_code_info.as_ref()?;
    let location = source
        .location
        .iter()
        .find(|location| location.path.as_slice() == path)?;
    let text = location.leading_comments.as_deref()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
