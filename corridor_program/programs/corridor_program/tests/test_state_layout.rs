use anchor_lang::{Discriminator, Space};
use corridor_program::state::{Config, IssuerEntry, Participant, QuoteReceipt};

// On-chain sizes from the v2 accounts table. A mismatch means a field type
// drifted from the spec. Config is 213 in the spec + 32 for genesis_hash.
#[test]
fn account_sizes_match_spec() {
    assert_eq!(Config::DISCRIMINATOR.len() + Config::INIT_SPACE, 245);
    assert_eq!(IssuerEntry::DISCRIMINATOR.len() + IssuerEntry::INIT_SPACE, 147);
    assert_eq!(Participant::DISCRIMINATOR.len() + Participant::INIT_SPACE, 139);
    assert_eq!(QuoteReceipt::DISCRIMINATOR.len() + QuoteReceipt::INIT_SPACE, 49);
}
