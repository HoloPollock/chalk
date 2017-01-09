use errors::*;
use fold::*;
use ir::*;
use solve::environment::{Environment, InEnvironment};
use solve::infer::{InferenceTable, ParameterInferenceVariable};
use solve::solver::Solver;
use solve::Solution;
use std::sync::Arc;

pub struct Prove<'s> {
    solver: &'s mut Solver,
    infer: InferenceTable,
    goals: Vec<InEnvironment<WhereClause>>,
}

enum Binding {
    ForAll(ParameterKind<UniverseIndex>),
    Exists(ParameterInferenceVariable),
}

impl<'s> Prove<'s> {
    pub fn new(solver: &'s mut Solver, goal: Box<Goal>) -> Self {
        let mut prove = Prove {
            solver: solver,
            infer: InferenceTable::new(),
            goals: vec![],
        };
        let environment = &Environment::new();
        prove.decompose(&goal, environment, &mut vec![]);
        prove
    }

    fn decompose(&mut self,
                 goal: &Goal,
                 environment: &Arc<Environment>,
                 bindings: &mut Vec<Binding>) {
        match *goal {
            Goal::Quantified(QuantifierKind::ForAll, ref parameter_kind, ref subgoal) => {
                let new_environment = environment.clone().new_universe();
                let parameter_universe = parameter_kind.map(|()| new_environment.universe);
                bindings.push(Binding::ForAll(parameter_universe));
                self.decompose(subgoal, &new_environment, bindings);
                bindings.pop().unwrap();
            }
            Goal::Quantified(QuantifierKind::Exists, ref parameter_kind, ref subgoal) => {
                let parameter_universe = parameter_kind.map(|()| environment.universe);
                let var = self.infer.new_parameter_variable(parameter_universe);
                bindings.push(Binding::Exists(var));
                self.decompose(subgoal, environment, bindings);
                bindings.pop().unwrap();
            }
            Goal::Implies(ref wc, ref subgoal) => {
                let wc = Subst::apply(&bindings, wc);
                let new_environment = &environment.add_clauses(wc);
                self.decompose(subgoal, new_environment, bindings);
            }
            Goal::And(ref subgoal1, ref subgoal2) => {
                self.decompose(subgoal1, environment, bindings);
                self.decompose(subgoal2, environment, bindings);
            }
            Goal::Leaf(ref wc) => {
                let wc = Subst::apply(&bindings, wc);
                self.goals.push(InEnvironment::new(environment, wc));
            }
        }
    }

    pub fn solve(mut self) -> Result<Solution<Vec<WhereClause>>> {
        let successful = self.solver.solve_all(&mut self.infer, self.goals.clone())?;
        let refined_goal = self.infer.constrained(self.goals
            .into_iter()
            .map(|g| g.goal)
            .collect::<Vec<_>>());
        let refined_goal = self.infer.quantify(&refined_goal);
        Ok(Solution {
            successful: successful,
            refined_goal: refined_goal,
        })
    }
}

struct Subst<'b> {
    bindings: &'b [Binding],
}

impl<'b> Subst<'b> {
    fn apply<T: Fold>(bindings: &[Binding], value: &T) -> T::Result {
        value.fold_with(&mut Subst { bindings: bindings }).unwrap()
    }
}

impl<'b> Folder for Subst<'b> {
    fn fold_var(&mut self, depth: usize) -> Result<Ty> {
        match self.bindings[depth] {
            Binding::ForAll(u) => {
                Ok(Ty::Apply(ApplicationTy {
                    name: TypeName::ForAll(u.ty().unwrap()),
                    parameters: vec![],
                }))
            }
            Binding::Exists(v) => Ok(v.ty().unwrap().to_ty()),
        }
    }

    fn fold_lifetime_var(&mut self, depth: usize) -> Result<Lifetime> {
        match self.bindings[depth] {
            Binding::ForAll(u) => Ok(Lifetime::ForAll(u.lifetime().unwrap())),
            Binding::Exists(v) => Ok(v.lifetime().unwrap().to_lifetime()),
        }
    }
}