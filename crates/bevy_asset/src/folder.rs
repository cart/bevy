use crate as bevy_asset;
use crate::{Asset, UntypedHandle};

#[derive(Asset)]
pub struct LoadedFolder {
    pub handles: Vec<UntypedHandle>,
}
