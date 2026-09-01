//! Registry-name identifier check (architecture §6, paired with the total
//! option-key lowering in `facts`): every extension declared in the vendored
//! `keryx/options.proto` must have a name that is a valid themelios identifier
//! (lowercase-initial), so a genuine keryx option always renders as an `opt/3`
//! constant. Catches a future mis-cased registry addition here, at CI, rather
//! than downstream as an `UnrenderableFacts` diagnostic.

mod support;

use prost_reflect::DescriptorPool;
use themelios_program::prelude::Name;

#[test]
fn every_vendored_option_name_is_a_themelios_identifier() {
    let bytes = support::compile_fixture("keryx/options.proto");
    let pool = DescriptorPool::decode(&bytes[..]).expect("the registry decodes");
    let file = pool
        .get_file_by_name("keryx/options.proto")
        .expect("the vendored file is in the pool");

    let extensions: Vec<_> = file.extensions().collect();
    assert!(!extensions.is_empty(), "the registry declares extensions");

    for extension in extensions {
        // Mirrors `descriptor::options::read`'s own key computation.
        let key = extension
            .full_name()
            .strip_prefix("keryx.")
            .unwrap_or_else(|| extension.full_name());
        assert!(
            Name::new(key).is_ok(),
            "registry option `{key}` is not a themelios identifier (must be lowercase-initial)"
        );
    }
}
