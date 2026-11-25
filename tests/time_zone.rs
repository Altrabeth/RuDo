//! The time zone database inside static musl builds, which often run on systems without
//! `/usr/share/zoneinfo`, so that "today" is the local day there too. Compiled for musl targets
//! only; `just test-musl` runs it.
#![cfg(target_env = "musl")]

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZoneDatabase};

#[test]
fn musl_builds_bundle_the_time_zone_database() {
    let zone_name = "Pacific/Kiritimati";
    let instant = Timestamp::from_second(1_764_000_000).unwrap();
    let expected_offset = Offset::constant(14);

    let system_under_test = TimeZoneDatabase::bundled().get(zone_name).unwrap();

    assert_eq!(system_under_test.to_offset(instant), expected_offset);
}
