use crate::error::{ArcWrenError, BudgetResource};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TurnBudget {
    pub max_iterations: u32,
    pub max_tool_calls: u32,
}

impl TurnBudget {
    #[must_use]
    pub const fn new(max_iterations: u32, max_tool_calls: u32) -> Self {
        Self {
            max_iterations,
            max_tool_calls,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetTracker {
    budget: TurnBudget,
    iterations: u32,
    tool_calls: u32,
}

impl BudgetTracker {
    #[must_use]
    pub const fn new(budget: TurnBudget) -> Self {
        Self {
            budget,
            iterations: 0,
            tool_calls: 0,
        }
    }

    #[must_use]
    pub const fn budget(&self) -> TurnBudget {
        self.budget
    }

    #[must_use]
    pub const fn iterations(&self) -> u32 {
        self.iterations
    }

    #[must_use]
    pub const fn tool_calls(&self) -> u32 {
        self.tool_calls
    }

    pub fn try_record_iteration(&mut self) -> Result<(), ArcWrenError> {
        Self::try_increment(
            &mut self.iterations,
            self.budget.max_iterations,
            BudgetResource::Iterations,
        )
    }

    pub fn try_record_tool_call(&mut self) -> Result<(), ArcWrenError> {
        Self::try_increment(
            &mut self.tool_calls,
            self.budget.max_tool_calls,
            BudgetResource::ToolCalls,
        )
    }

    fn try_increment(
        current: &mut u32,
        limit: u32,
        resource: BudgetResource,
    ) -> Result<(), ArcWrenError> {
        if *current >= limit {
            return Err(ArcWrenError::BudgetExceeded { resource, limit });
        }

        *current = current
            .checked_add(1)
            .ok_or(ArcWrenError::BudgetExceeded { resource, limit })?;
        Ok(())
    }
}
