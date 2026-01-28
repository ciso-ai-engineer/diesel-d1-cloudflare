use diesel::query_builder::{AstPass, IntoBoxedClause, QueryFragment};
use diesel::query_builder::{BoxedLimitOffsetClause, LimitOffsetClause};
use diesel::query_builder::{LimitClause, NoLimitClause};
use diesel::query_builder::{NoOffsetClause, OffsetClause};
use diesel::result::QueryResult;

use crate::backend::D1Backend;

impl QueryFragment<D1Backend> for LimitOffsetClause<NoLimitClause, NoOffsetClause> {
    fn walk_ast<'b>(&'b self, _out: AstPass<'_, 'b, D1Backend>) -> QueryResult<()> {
        Ok(())
    }
}

impl<L> QueryFragment<D1Backend> for LimitOffsetClause<LimitClause<L>, NoOffsetClause>
where
    LimitClause<L>: QueryFragment<D1Backend>,
{
    fn walk_ast<'b>(&'b self, out: AstPass<'_, 'b, D1Backend>) -> QueryResult<()> {
        self.limit_clause.walk_ast(out)?;
        Ok(())
    }
}

impl<O> QueryFragment<D1Backend> for LimitOffsetClause<NoLimitClause, OffsetClause<O>>
where
    OffsetClause<O>: QueryFragment<D1Backend>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, D1Backend>) -> QueryResult<()> {
        // D1Backend requires a limit clause in front of any offset clause
        // using `LIMIT -1` is the same as not having any limit clause
        // https://D1Backend.org/lang_select.html
        out.push_sql(" LIMIT -1 ");
        self.offset_clause.walk_ast(out)?;
        Ok(())
    }
}

impl<L, O> QueryFragment<D1Backend> for LimitOffsetClause<LimitClause<L>, OffsetClause<O>>
where
    LimitClause<L>: QueryFragment<D1Backend>,
    OffsetClause<O>: QueryFragment<D1Backend>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, D1Backend>) -> QueryResult<()> {
        self.limit_clause.walk_ast(out.reborrow())?;
        self.offset_clause.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl QueryFragment<D1Backend> for BoxedLimitOffsetClause<'_, D1Backend> {
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, D1Backend>) -> QueryResult<()> {
        match (self.limit.as_ref(), self.offset.as_ref()) {
            (Some(limit), Some(offset)) => {
                limit.walk_ast(out.reborrow())?;
                offset.walk_ast(out.reborrow())?;
            }
            (Some(limit), None) => {
                limit.walk_ast(out.reborrow())?;
            }
            (None, Some(offset)) => {
                // See the `QueryFragment` implementation for `LimitOffsetClause` for details.
                out.push_sql(" LIMIT -1 ");
                offset.walk_ast(out.reborrow())?;
            }
            (None, None) => {}
        }
        Ok(())
    }
}

// Have explicit impls here because we need to set `Some`/`None` for the clauses
// correspondingly, otherwise we cannot match on it in the `QueryFragment` impl
// above
impl<'a> IntoBoxedClause<'a, D1Backend> for LimitOffsetClause<NoLimitClause, NoOffsetClause> {
    type BoxedClause = BoxedLimitOffsetClause<'a, D1Backend>;

    fn into_boxed(self) -> Self::BoxedClause {
        BoxedLimitOffsetClause {
            limit: None,
            offset: None,
        }
    }
}

impl<'a, L> IntoBoxedClause<'a, D1Backend> for LimitOffsetClause<LimitClause<L>, NoOffsetClause>
where
    L: QueryFragment<D1Backend> + Send + 'a,
{
    type BoxedClause = BoxedLimitOffsetClause<'a, D1Backend>;

    fn into_boxed(self) -> Self::BoxedClause {
        BoxedLimitOffsetClause {
            limit: Some(Box::new(self.limit_clause)),
            offset: None,
        }
    }
}

impl<'a, O> IntoBoxedClause<'a, D1Backend> for LimitOffsetClause<NoLimitClause, OffsetClause<O>>
where
    O: QueryFragment<D1Backend> + Send + 'a,
{
    type BoxedClause = BoxedLimitOffsetClause<'a, D1Backend>;

    fn into_boxed(self) -> Self::BoxedClause {
        BoxedLimitOffsetClause {
            limit: None,
            offset: Some(Box::new(self.offset_clause)),
        }
    }
}

impl<'a, L, O> IntoBoxedClause<'a, D1Backend> for LimitOffsetClause<LimitClause<L>, OffsetClause<O>>
where
    L: QueryFragment<D1Backend> + Send + 'a,
    O: QueryFragment<D1Backend> + Send + 'a,
{
    type BoxedClause = BoxedLimitOffsetClause<'a, D1Backend>;

    fn into_boxed(self) -> Self::BoxedClause {
        BoxedLimitOffsetClause {
            limit: Some(Box::new(self.limit_clause)),
            offset: Some(Box::new(self.offset_clause)),
        }
    }
}
