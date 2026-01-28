use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

use diesel::row::{Field, PartialRow, Row, RowIndex, RowSealed};
use wasm_bindgen::JsValue;

use crate::{backend::D1Backend, value::D1Value};

pub struct D1Row {
    _js_obj: Rc<RefCell<JsValue>>,
    field_vec: Vec<String>,
}

// SAFETY: this is safe under WASM and workers because there's no threads and therefore no race conditions (at least memory ones)
unsafe impl Send for D1Row {}
unsafe impl Sync for D1Row {}

impl D1Row {
    pub fn new(js_value: JsValue, field_vec: Vec<String>) -> Self {
        Self {
            // again
            _js_obj: Rc::new(RefCell::new(js_value)),
            field_vec,
        }
    }
}

impl RowSealed for D1Row {}

impl<'stmt> Row<'stmt, D1Backend> for D1Row {
    type Field<'f>
        = D1Field<'f>
    where
        'stmt: 'f,
        Self: 'f;

    type InnerPartialRow = Self;

    fn field_count(&self) -> usize {
        self.field_vec.len()
    }

    fn get<'b, I>(&'b self, idx: I) -> Option<Self::Field<'b>>
    where
        'stmt: 'b,
        Self: diesel::row::RowIndex<I>,
    {
        let index = self.idx(idx)?;
        let name = self.field_vec.get(index)?;
        Some(D1Field {
            name: name.to_string(),
            row: self._js_obj.borrow(),
        })
    }

    fn partial_row(
        &self,
        range: std::ops::Range<usize>,
    ) -> diesel::row::PartialRow<'_, Self::InnerPartialRow> {
        PartialRow::new(self, range)
    }
}

impl RowIndex<usize> for D1Row {
    fn idx(&self, idx: usize) -> Option<usize> {
        if idx < self.field_vec.len() {
            Some(idx)
        } else {
            None
        }
    }
}
// TODO(lduarte): it's not efficient to do it like this for now ahah, but JS
impl RowIndex<&str> for D1Row {
    fn idx(&self, field: &str) -> Option<usize> {
        self.field_vec.iter().position(|i| i == field)
    }
}

pub struct D1Field<'stmt> {
    row: Ref<'stmt, JsValue>,
    name: String,
}

impl<'stmt> Field<'stmt, D1Backend> for D1Field<'stmt> {
    fn field_name(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn value(&self) -> Option<D1Value> {
        let js_value = js_sys::Reflect::get(&self.row, &self.name.clone().into()).ok()?;

        Some(D1Value::new(js_value))
    }
}
