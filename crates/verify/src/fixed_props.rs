//! Bolero property tests for `FixedI64`.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_fixed::FixedI64;

    type F6 = FixedI64<6>;

    #[test]
    fn checked_add_commutative() {
        check!().with_type::<(i64, i64)>().for_each(|(a, b)| {
            let fa = F6::from_raw(*a);
            let fb = F6::from_raw(*b);
            assert_eq!(fa.checked_add(fb), fb.checked_add(fa));
        });
    }

    #[test]
    fn checked_sub_inverse_of_add() {
        check!().with_type::<(i64, i64)>().for_each(|(a, b)| {
            let fa = F6::from_raw(*a);
            let fb = F6::from_raw(*b);
            if let Some(sum) = fa.checked_add(fb) {
                assert_eq!(sum.checked_sub(fb), Some(fa));
            }
        });
    }

    #[test]
    fn checked_mul_trunc_commutative() {
        check!().with_type::<(i64, i64)>().for_each(|(a, b)| {
            let fa = F6::from_raw(*a);
            let fb = F6::from_raw(*b);
            assert_eq!(fa.checked_mul_trunc(fb), fb.checked_mul_trunc(fa));
        });
    }

    #[test]
    fn from_int_to_raw_consistency() {
        check!().with_type::<i64>().for_each(|n| {
            if let Some(f) = F6::from_int(*n) {
                assert_eq!(f.to_raw(), *n * F6::SCALE);
            }
        });
    }

    #[test]
    fn rescale_widen_then_narrow_preserves() {
        check!().with_type::<i64>().for_each(|raw| {
            let f2 = FixedI64::<2>::from_raw(*raw);
            if let Some(f6) = f2.rescale_trunc::<6>() {
                if let Some(back) = f6.rescale_trunc::<2>() {
                    assert_eq!(back, f2);
                }
            }
        });
    }

    #[test]
    fn checked_rescale_exact_none_iff_lossy() {
        check!().with_type::<i64>().for_each(|raw| {
            let f6 = F6::from_raw(*raw);
            let exact: Option<FixedI64<2>> = f6.checked_rescale_exact();
            let trunc: Option<FixedI64<2>> = f6.rescale_trunc();

            match (exact, trunc) {
                (Some(e), Some(t)) => {
                    assert_eq!(e, t);
                    let remainder = raw % 10_000;
                    assert_eq!(remainder, 0);
                }
                (None, Some(_)) => {
                    let remainder = raw % 10_000;
                    assert_ne!(remainder, 0);
                }
                (None, None) | (Some(_), None) => {}
            }
        });
    }

    #[test]
    fn display_parse_roundtrip() {
        check!().with_type::<i64>().for_each(|raw| {
            let f = F6::from_raw(*raw);
            let s = std::format!("{f}");
            let parsed = F6::from_str_decimal(&s);
            assert_eq!(parsed, Ok(f), "round-trip failed for raw={raw}");
        });
    }
}
