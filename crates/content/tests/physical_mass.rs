//! File admission uses the same validated physical mass as direct engine calls.
use arpg_sim::{Mass, Template};

#[test]
fn body_mass_roundtrips_and_invalid_file_values_are_refused() {
    let body = Template::BODY.with_mass(Mass::try_from(4.0).unwrap());
    let written = ron::to_string(&body).unwrap();
    assert_eq!(ron::from_str::<Template>(&written).unwrap(), body);
    assert_eq!(ron::from_str::<Template>("()").unwrap().mass(), Mass::UNIT);
    for mass in ["NaN", "inf", "-inf", "0.0", "-1.0", "0.05", "10001.0"] {
        assert!(
            ron::from_str::<Template>(&format!("(mass:{mass})")).is_err(),
            "admitted {mass}"
        );
    }
}
