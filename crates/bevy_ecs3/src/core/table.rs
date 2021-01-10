use crate::core::{BlobVec, ComponentId, Entity, SparseSet, TypeInfo};

pub struct TableId(usize);

pub struct Column {
    component_id: ComponentId,
    data: BlobVec,
}

#[derive(Default)]
pub struct Tables {
    table: Vec<Table>,
}

impl Tables {}

pub struct Table {
    columns: SparseSet<ComponentId, Column>,
    entities: Vec<Entity>,
    capacity: usize,
    grow_amount: usize,
}

impl Table {
    pub fn new(capacity: usize, column_capacity: usize, grow_amount: usize) -> Table {
        Self {
            columns: SparseSet::new(column_capacity),
            entities: Vec::new(),
            capacity,
            grow_amount,
        }
    }

    pub fn add_component_column(&mut self, component_id: ComponentId, type_info: &TypeInfo) {
        self.columns.insert(
            component_id,
            Column {
                component_id,
                data: BlobVec::new(type_info.layout(), type_info.drop(), self.capacity),
            },
        )
    }

    /// SAFETY: assumes data has already been allocated for the given
    pub unsafe fn put_component_unchecked(
        &mut self,
        component_id: ComponentId,
        row: usize,
        data: *mut u8,
    ) {
        let component_column = self.get_component_column_unchecked(component_id);
        component_column.set_unchecked(row, data);
    }

    /// Removes the entity at the given row and returns the entity swapped in to replace it
    /// SAFETY: `row` must be in-bounds
    pub unsafe fn swap_remove(&mut self, row: usize) -> Option<Entity> {
        for column in self.columns.values_mut() {
            column.data.swap_remove_unchecked(row);
        }

        self.entities.swap_remove(row);
        // if the last row was removed, no swap occurred
        if row == self.len() {
            None
        } else {
            Some(self.entities[row])
        }
    }

    /// Moves the `row` column values to `new_table`, for the columns shared between both tables
    /// SAFETY: row must be in-bounds
    pub unsafe fn move_to(&mut self, row: usize, new_table: &mut Table) {
        let entity = self.entities.swap_remove(row);
        let new_row = new_table.allocate(entity);

        for column in self.columns.values_mut() {
            if let Some(new_column) = new_table.get_component_column_mut(column.component_id) {
                let data = column.data.swap_remove_and_forget_unchecked(row);
                new_column.set_unchecked(new_row, data);
            } else {
                column.data.swap_remove_unchecked(row);
            }
        }
    }

    /// SAFETY: a column with the given `component_id` must exist
    #[inline]
    unsafe fn get_component_column_unchecked(&mut self, component_id: ComponentId) -> &mut BlobVec {
        &mut self.columns.get_unchecked_mut(component_id).data
    }

    #[inline]
    pub fn get_component_column(&self, component_id: ComponentId) -> Option<&BlobVec> {
        self.columns.get(component_id).map(|c| &c.data)
    }

    #[inline]
    pub fn get_component_column_mut(&mut self, component_id: ComponentId) -> Option<&mut BlobVec> {
        self.columns.get_mut(component_id).map(|c| &mut c.data)
    }

    #[inline]
    pub fn has_component_column(&self, component_id: ComponentId) -> bool {
        self.columns.contains(component_id)
    }

    pub fn grow(&mut self, amount: usize) {
        let new_capacity = self.capacity + amount;
        for column in self.columns.values_mut() {
            column.data.grow(amount);
        }
        self.capacity = new_capacity;
    }

    pub fn allocate(&mut self, entity: Entity) -> usize {
        if self.len() == self.capacity {
            self.grow(self.grow_amount);
        }

        self.entities.push(entity);
        self.entities.len() - 1
    }

    #[inline]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }
}
