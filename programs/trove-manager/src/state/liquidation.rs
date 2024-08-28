#[derive(Default)]
pub struct LiquidationTotals {
    pub total_coll_in_sequence: u64,
    pub total_debt_in_sequence: u64,
    pub total_coll_gas_compensation: u64,
    pub total_usv_gas_compensation: u64,
    pub total_debt_to_offset: u64,
    pub total_coll_to_send_to_sp: u64,
    pub total_debt_to_redistribute: u64,
    pub total_coll_to_redistribute: u64,
    pub total_coll_surplus: u64,
}

#[derive(Default)]
pub struct LiquidationValues {
    pub entire_trove_debt: u64,
    pub entire_trove_coll: u64,
    pub coll_gas_compensation: u64,
    pub usv_gas_compensation: u64,
    pub debt_to_offset: u64,
    pub coll_to_send_to_sp: u64,
    pub debt_to_redistribute: u64,
    pub coll_to_redistribute: u64,
    pub coll_surplus: u64,
}

#[derive(Default)]
pub struct LocalVariablesLiquidationSequence {
    pub remaining_usv_in_stab_pool: u64,
    pub icr: u64,
    pub back_to_normal_mode: bool,
    pub entire_system_debt: u64,
    pub entire_system_coll: u64,
}

impl LiquidationValues {
    pub fn offset_and_redistribute(&mut self, coll: u64, usv_in_stab_pool: u64) {
        let mut debt_to_offset: u64 = 0;
        let mut coll_to_send_to_sp: u64 = 0;
        let debt_to_redistribute: u64;
        let coll_to_redistribute: u64;

        if usv_in_stab_pool > 0 {
            debt_to_offset = std::cmp::min(self.entire_trove_debt, usv_in_stab_pool);
            coll_to_send_to_sp = u64::try_from(
                (coll as u128)
                    .checked_mul(debt_to_offset.into())
                    .unwrap()
                    .checked_div(self.entire_trove_debt.into())
                    .unwrap(),
            )
            .unwrap();
            debt_to_redistribute = self.entire_trove_debt.checked_sub(debt_to_offset).unwrap();
            coll_to_redistribute = coll.checked_sub(coll_to_send_to_sp).unwrap();
        } else {
            debt_to_redistribute = self.entire_trove_debt;
            coll_to_redistribute = coll;
        }

        self.debt_to_offset = debt_to_offset;
        self.coll_to_send_to_sp = coll_to_send_to_sp;
        self.debt_to_redistribute = debt_to_redistribute;
        self.coll_to_redistribute = coll_to_redistribute;
    }
}

impl LiquidationTotals {
    pub fn add_liquidation_values(&mut self, single_liquidation: &LiquidationValues) {
        self.total_coll_gas_compensation = self
            .total_coll_gas_compensation
            .checked_add(single_liquidation.coll_gas_compensation)
            .unwrap();
        self.total_usv_gas_compensation = self
            .total_usv_gas_compensation
            .checked_add(single_liquidation.usv_gas_compensation)
            .unwrap();
        self.total_debt_in_sequence = self
            .total_debt_in_sequence
            .checked_add(single_liquidation.entire_trove_debt)
            .unwrap();
        self.total_coll_in_sequence = self
            .total_coll_in_sequence
            .checked_add(single_liquidation.entire_trove_coll)
            .unwrap();
        self.total_debt_to_offset = self
            .total_debt_to_offset
            .checked_add(single_liquidation.debt_to_offset)
            .unwrap();
        self.total_coll_to_send_to_sp = self
            .total_coll_to_send_to_sp
            .checked_add(single_liquidation.coll_to_send_to_sp)
            .unwrap();
        self.total_debt_to_redistribute = self
            .total_debt_to_redistribute
            .checked_add(single_liquidation.debt_to_redistribute)
            .unwrap();
        self.total_coll_to_redistribute = self
            .total_coll_to_redistribute
            .checked_add(single_liquidation.coll_to_redistribute)
            .unwrap();
        self.total_coll_surplus = self
            .total_coll_surplus
            .checked_add(single_liquidation.coll_surplus)
            .unwrap();
    }
}

impl LocalVariablesLiquidationSequence {
    pub fn update(&mut self, single_liquidation: &LiquidationValues) {
        self.remaining_usv_in_stab_pool = self
            .remaining_usv_in_stab_pool
            .checked_sub(single_liquidation.debt_to_offset)
            .unwrap();
        self.entire_system_debt = self
            .entire_system_debt
            .checked_sub(single_liquidation.debt_to_offset)
            .unwrap();
        self.entire_system_coll = self
            .entire_system_coll
            .checked_sub(single_liquidation.coll_to_send_to_sp)
            .unwrap()
            .checked_sub(single_liquidation.coll_gas_compensation)
            .unwrap()
            .checked_sub(single_liquidation.coll_surplus)
            .unwrap();
    }
}
