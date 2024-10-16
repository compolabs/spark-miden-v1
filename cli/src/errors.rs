#[derive(Debug, PartialEq, Eq)]
pub enum OrderError {
    AssetsNotMatching,
    TooFewSourceAssets,
    TooManyTargetAssets,
    // MissingOrderId,
}
