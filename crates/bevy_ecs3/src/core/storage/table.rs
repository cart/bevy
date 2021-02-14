use crate::core::{
    ArchetypeId, BlobVec, ComponentFlags, ComponentId, ComponentInfo, Components, Entity, SparseSet,
};
use bevy_utils::{AHasher, HashMap};
use std::{
    cell::UnsafeCell,
    hash::{Hash, Hasher},
    ptr::NonNull,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableId(usize);

impl TableId {
    #[inline]
    pub fn new(index: usize) -> Self {
        TableId(index)
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0
    }

    #[inline]
    pub const fn empty_table() -> TableId {
        TableId(0)
    }
}

pub struct Column {
    pub(crate) component_id: ComponentId,
    pub(crate) data: BlobVec,
    pub(crate) flags: UnsafeCell<Vec<ComponentFlags>>,
}

impl Column {
    #[inline]
    pub fn with_capacity(component_info: &ComponentInfo, capacity: usize) -> Self {
        Column {
            component_id: component_info.id(),
            data: BlobVec::new(component_info.layout(), component_info.drop(), capacity),
            flags: UnsafeCell::new(Vec::with_capacity(capacity)),
        }
    }

    /// SAFETY: assumes data has already been allocated for the given row/column.
    #[inline]
    pub unsafe fn set_unchecked(&self, row: usize, data: *mut u8) {
        self.data.set_unchecked(row, data);
    }

    #[inline]
    pub unsafe fn get_flags_unchecked_mut(&self, row: usize) -> &mut ComponentFlags {
        (*self.flags.get()).get_unchecked_mut(row)
    }

    #[inline]
    pub(crate) unsafe fn swap_remove_unchecked(&mut self, row: usize) {
        self.data.swap_remove_unchecked(row);
        (*self.flags.get()).swap_remove(row);
    }

    #[inline]
    pub(crate) unsafe fn swap_remove_and_forget_unchecked(
        &mut self,
        row: usize,
    ) -> (*mut u8, ComponentFlags) {
        let data = self.data.swap_remove_and_forget_unchecked(row);
        let flags = (*self.flags.get()).swap_remove(row);
        (data, flags)
    }

    /// SAFETY: allocated value must be immediately set
    pub(crate) unsafe fn push_uninit(&mut self) {
        self.data.push_uninit();
        (*self.flags.get()).push(ComponentFlags::empty());
    }

    #[inline]
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.data.grow(additional);
        // SAFE: unique access to self
        unsafe {
            let flags = &mut (*self.flags.get());
            flags.reserve(additional);
        }
    }

    /// SAFETY: must ensure rust mutability rules are not violated
    #[inline]
    pub unsafe fn get_ptr(&self) -> NonNull<u8> {
        self.data.get_ptr()
    }

    /// SAFETY: must ensure rust mutability rules are not violated
    #[inline]
    pub unsafe fn get_flags_mut_ptr(&self) -> *mut ComponentFlags {
        (*self.flags.get()).as_mut_ptr()
    }

    /// SAFETY: must ensure rust mutability rules are not violated
    #[inline]
    pub unsafe fn get_unchecked(&self, row: usize) -> *mut u8 {
        self.data.get_unchecked(row)
    }

    /// SAFETY: must ensure rust mutability rules are not violated
    #[inline]
    pub unsafe fn get_flags_unchecked(&self, row: usize) -> *mut ComponentFlags {
        self.get_flags_mut_ptr().add(row)
    }

    #[inline]
    pub(crate) fn clear_flags(&mut self) {
        let flags = unsafe { (*self.flags.get()).iter_mut() };
        for component_flags in flags {
            *component_flags = ComponentFlags::empty();
        }
    }
}

pub struct Tables {
    tables: Vec<Table>,
    table_ids: HashMap<u64, TableId>,
}

impl Default for Tables {
    fn default() -> Self {
        let empty_table = Table::with_capacity(0, 0, 64);
        Tables {
            tables: vec![empty_table],
            table_ids: HashMap::default(),
        }
    }
}

pub struct TableMoveResult {
    pub swapped_entity: Option<Entity>,
    pub new_row: usize,
}

impl Tables {
    #[inline]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, id: TableId) -> &mut Table {
        self.tables.get_unchecked_mut(id.index())
    }

    #[inline]
    pub unsafe fn get_unchecked(&self, id: TableId) -> &Table {
        self.tables.get_unchecked(id.index())
    }

    /// SAFETY: `a` and `b` must both be valid tables and they _must_ be different
    #[inline]
    pub(crate) unsafe fn get_2_mut_unchecked(
        &mut self,
        a: TableId,
        b: TableId,
    ) -> (&mut Table, &mut Table) {
        let ptr = self.tables.as_mut_ptr();
        (&mut *ptr.add(a.index()), &mut *ptr.add(b.index()))
    }

    // SAFETY: `component_ids` must contain components that exist in `components`
    pub unsafe fn get_id_or_insert(
        &mut self,
        component_ids: &[ComponentId],
        components: &Components,
    ) -> TableId {
        let mut hasher = AHasher::default();
        component_ids.hash(&mut hasher);
        let hash = hasher.finish();
        let tables = &mut self.tables;
        *self.table_ids.entry(hash).or_insert_with(move || {
            let mut table = Table::with_capacity(0, component_ids.len(), 64);
            for component_id in component_ids.iter() {
                table.add_column(components.get_info_unchecked(*component_id));
            }
            tables.push(table);
            TableId(tables.len() - 1)
        })
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Table> {
        self.tables.iter()
    }

    pub(crate) fn clear_flags(&mut self) {
        for table in self.tables.iter_mut() {
            table.clear_flags();
        }
    }
}

pub struct Table {
    columns: SparseSet<ComponentId, Column>,
    entities: Vec<Entity>,
    archetypes: Vec<ArchetypeId>,
    grow_amount: usize,
    capacity: usize,
}

impl Table {
    pub const fn new(grow_amount: usize) -> Table {
        Self {
            columns: SparseSet::new(),
            entities: Vec::new(),
            archetypes: Vec::new(),
            grow_amount,
            capacity: 0,
        }
    }

    pub fn with_capacity(capacity: usize, column_capacity: usize, grow_amount: usize) -> Table {
        Self {
            columns: SparseSet::with_capacity(column_capacity),
            entities: Vec::with_capacity(capacity),
            archetypes: Vec::new(),
            grow_amount,
            capacity,
        }
    }

    #[inline]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn add_archetype(&mut self, archetype_id: ArchetypeId) {
        self.archetypes.push(archetype_id);
    }

    pub fn add_column(&mut self, component_info: &ComponentInfo) {
        self.columns.insert(
            component_info.id(),
            Column::with_capacity(component_info, self.capacity()),
        )
    }

    /// Removes the entity at the given row and returns the entity swapped in to replace it (if an entity was swapped in)
    /// SAFETY: `row` must be in-bounds
    pub unsafe fn swap_remove(&mut self, row: usize) -> Option<Entity> {
        for column in self.columns.values_mut() {
            column.swap_remove_unchecked(row);
        }
        let is_last = row == self.entities.len() - 1;
        self.entities.swap_remove(row);
        if is_last {
            None
        } else {
            Some(self.entities[row])
        }
    }

    /// Moves the `row` column values to `new_table`, for the columns shared between both tables. Returns the index of the
    /// new row in `new_table` and the entity in this table swapped in to replace it (if an entity was swapped in).
    /// SAFETY: row must be in-bounds. missing columns will be "forgotten". it is the caller's responsibility to drop it
    pub unsafe fn move_to_and_forget_missing_unchecked(
        &mut self,
        row: usize,
        new_table: &mut Table,
    ) -> TableMoveResult {
        let is_last = row == self.entities.len() - 1;
        let new_row = new_table.allocate(self.entities.swap_remove(row));
        for column in self.columns.values_mut() {
            let (data, flags) = column.swap_remove_and_forget_unchecked(row);
            if let Some(new_column) = new_table.get_column_mut(column.component_id) {
                new_column.set_unchecked(new_row, data);
                *new_column.get_flags_unchecked_mut(new_row) = flags;
            }
        }
        TableMoveResult {
            new_row,
            swapped_entity: if is_last {
                None
            } else {
                Some(self.entities[row])
            },
        }
    }

    /// Moves the `row` column values to `new_table`, for the columns shared between both tables. Returns the index of the
    /// new row in `new_table` and the entity in this table swapped in to replace it (if an entity was swapped in).
    /// SAFETY: row must be in-bounds
    pub unsafe fn move_to_and_drop_missing_unchecked(
        &mut self,
        row: usize,
        new_table: &mut Table,
    ) -> TableMoveResult {
        let is_last = row == self.entities.len() - 1;
        let new_row = new_table.allocate(self.entities.swap_remove(row));
        for column in self.columns.values_mut() {
            if let Some(new_column) = new_table.get_column_mut(column.component_id) {
                let (data, flags) = column.swap_remove_and_forget_unchecked(row);
                new_column.set_unchecked(new_row, data);
                *new_column.get_flags_unchecked_mut(new_row) = flags;
            } else {
                column.swap_remove_unchecked(row);
            }
        }
        TableMoveResult {
            new_row,
            swapped_entity: if is_last {
                None
            } else {
                Some(self.entities[row])
            },
        }
    }

    /// Moves the `row` column values to `new_table`, for the columns shared between both tables. Returns the index of the
    /// new row in `new_table` and the entity in this table swapped in to replace it (if an entity was swapped in).
    /// SAFETY: `row` must be in-bounds. `new_table` must contain every component this table has
    pub unsafe fn move_to_superset_unchecked(
        &mut self,
        row: usize,
        new_table: &mut Table,
    ) -> TableMoveResult {
        let is_last = row == self.entities.len() - 1;
        let new_row = new_table.allocate(self.entities.swap_remove(row));
        for column in self.columns.values_mut() {
            let new_column = new_table.get_column_unchecked_mut(column.component_id);
            let (data, flags) = column.swap_remove_and_forget_unchecked(row);
            new_column.set_unchecked(new_row, data);
            *new_column.get_flags_unchecked_mut(new_row) = flags;
        }
        TableMoveResult {
            new_row,
            swapped_entity: if is_last {
                None
            } else {
                Some(self.entities[row])
            },
        }
    }

    /// SAFETY: a column with the given `component_id` must exist
    #[inline]
    pub unsafe fn get_column_unchecked(&self, component_id: ComponentId) -> &Column {
        self.columns.get_unchecked(component_id)
    }

    /// SAFETY: a column with the given `component_id` must exist
    #[inline]
    pub unsafe fn get_column_unchecked_mut(&mut self, component_id: ComponentId) -> &mut Column {
        self.columns.get_unchecked_mut(component_id)
    }

    #[inline]
    pub fn get_column(&self, component_id: ComponentId) -> Option<&Column> {
        self.columns.get(component_id)
    }

    #[inline]
    pub fn get_column_mut(&mut self, component_id: ComponentId) -> Option<&mut Column> {
        self.columns.get_mut(component_id)
    }

    #[inline]
    pub fn has_column(&self, component_id: ComponentId) -> bool {
        self.columns.contains(component_id)
    }

    pub fn reserve(&mut self, amount: usize) {
        for column in self.columns.values_mut() {
            column.reserve(amount);
        }
        self.entities.reserve(amount);
        self.capacity += amount;
    }

    /// Allocates space for a new entity
    /// SAFETY: the allocated row must be written to immediately with valid values in each column
    pub unsafe fn allocate(&mut self, entity: Entity) -> usize {
        if self.len() == self.capacity() {
            self.reserve(self.grow_amount);
        }

        let index = self.entities.len();
        self.entities.push(entity);
        for column in self.columns.values_mut() {
            column.data.set_len(self.entities.len());
            (*column.flags.get()).push(ComponentFlags::empty());
        }
        index
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub(crate) fn clear_flags(&mut self) {
        for column in self.columns.values_mut() {
            column.clear_flags();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Column> {
        self.columns.values()
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{Components, Entity, Table, TypeInfo};

    #[test]
    fn table() {
        let mut components = Components::default();
        let type_info = TypeInfo::of::<usize>();
        let component_id = components.get_or_insert_with(type_info.type_id(), || type_info);
        let columns = &[component_id];
        let mut table = Table::with_capacity(0, columns.len(), 64);
        table.add_column(components.get_info(component_id).unwrap());
        let entities = (0..200).map(|i| Entity::new(i)).collect::<Vec<_>>();
        for (row, entity) in entities.iter().cloned().enumerate() {
            unsafe {
                table.allocate(entity);
                let mut value = row;
                let value_ptr = ((&mut value) as *mut usize).cast::<u8>();
                table
                    .get_column_unchecked(component_id)
                    .set_unchecked(row, value_ptr);
            };
        }

        assert_eq!(table.capacity(), 256);
        assert_eq!(table.len(), 200);
    }
}
