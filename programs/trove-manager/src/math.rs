use crate::constants::{DECIMAL_PRECISION, NICR_PRECISION};

pub fn dec_mul(x: u64, y: u64) -> Option<u64> {
    let prod_xy = x.checked_mul(y).unwrap();
    prod_xy
        .checked_add(DECIMAL_PRECISION / 2)?
        .checked_div(DECIMAL_PRECISION)
}

pub fn dec_pow(base: u64, minutes: u64) -> Option<u64> {
    let mut capped_minutes = minutes;
    if minutes > 525600000 {
        capped_minutes = 525600000;
    }
    if minutes == 0 {
        return Some(DECIMAL_PRECISION);
    }

    let mut y = DECIMAL_PRECISION;
    let mut x = base;
    let mut n = capped_minutes;

    while n > 1 {
        if n % 2 == 0 {
            x = dec_mul(x, x)?;
            n = n.checked_div(2)?;
        } else {
            y = dec_mul(x, y)?;
            x = dec_mul(x, x)?;
            n = n.checked_sub(1)?.checked_div(2)?;
        }
    }
    dec_mul(x, y)
}

pub fn compute_cr(coll: u64, debt: u64, price: u64) -> Option<u64> {
    if debt > 0 {
        return u64::try_from(
            (coll as u128)
                .checked_mul(price.into())?
                .checked_div(debt.into())?,
        )
        .ok();
    }
    Some(u64::MAX)
}

pub fn compute_nominal_cr(coll: u64, debt: u64) -> Option<u64> {
    if debt > 0 {
        return u64::try_from(
            (coll as u128)
                .checked_mul(NICR_PRECISION.into())?
                .checked_div(debt.into())?,
        )
        .ok();
    }
    Some(u64::MAX)
}
