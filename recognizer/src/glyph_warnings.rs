//! Direct port of service/vendor/wha/src/parser/glyphWarnings.js.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphWarning {
    NoRingDetected,
    RingIncomplete,
    UnsupportedNestedRing,
    UnsupportedMultipleRings,
    UnsupportedMultipleSigils,
    MissingPrimarySigil,
    PrimarySigilConfidenceLow,
    PrimarySigilAmbiguous,
    PrimaryElementMissing,
    PrimaryElementUnsupported,
    CenterUnknownContamination,
    SymbolNearLayerBoundary,
    SymbolContaminated,
    SymbolAmbiguous,
    SymbolMessy,
}

impl GlyphWarning {
    pub fn as_str(self) -> &'static str {
        match self {
            GlyphWarning::NoRingDetected => "no_ring_detected",
            GlyphWarning::RingIncomplete => "ring_incomplete",
            GlyphWarning::UnsupportedNestedRing => "unsupported_nested_ring",
            GlyphWarning::UnsupportedMultipleRings => "unsupported_multiple_rings",
            GlyphWarning::UnsupportedMultipleSigils => "unsupported_multiple_sigils",
            GlyphWarning::MissingPrimarySigil => "missing_primary_sigil",
            GlyphWarning::PrimarySigilConfidenceLow => "primary_sigil_confidence_low",
            GlyphWarning::PrimarySigilAmbiguous => "primary_sigil_ambiguous",
            GlyphWarning::PrimaryElementMissing => "primary_element_missing",
            GlyphWarning::PrimaryElementUnsupported => "primary_element_unsupported",
            GlyphWarning::CenterUnknownContamination => "center_unknown_contamination",
            GlyphWarning::SymbolNearLayerBoundary => "symbol_near_layer_boundary",
            GlyphWarning::SymbolContaminated => "symbol_contaminated",
            GlyphWarning::SymbolAmbiguous => "symbol_ambiguous",
            GlyphWarning::SymbolMessy => "symbol_messy",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_strings_match_js_exactly() {
        assert_eq!(GlyphWarning::NoRingDetected.as_str(), "no_ring_detected");
        assert_eq!(GlyphWarning::SymbolMessy.as_str(), "symbol_messy");
    }
}
