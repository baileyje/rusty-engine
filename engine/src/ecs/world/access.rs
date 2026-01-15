//! Access control and permission tracking for ECS world resources.
//!
//! This module provides the [`Access`] type for representing and validating access to world
//! resources (components and the world itself). It's designed to support a scheduler
//! that can analyze system requirements and detect conflicts.
//!
//! # Overview
//!
//! The access system uses a **hierarchical permission model**:
//! - **Mutable world access** - grants everything (all components, any mutability)
//! - **Immutable world access** - grants immutable world and immutable components
//! - **Component access** - grants specific components with specified mutability
//!
//! # Conflict Detection
//!
//! Two accesses conflict if they cannot be held simultaneously, following Rust's aliasing rules:
//! - Multiple immutable accesses to the same resource are OK
//! - Mutable access conflicts with any other access (mutable or immutable) to the same resource
//!
//! ```rust,ignore
//! use rusty_engine::ecs::world::access::Access;
//! use rusty_engine::ecs::component::Spec;
//!
//! let read_pos = Access::to_components(Spec::new([pos_id]), Spec::EMPTY);
//! let write_pos = Access::to_components(Spec::EMPTY, Spec::new([pos_id]));
//! let write_vel = Access::to_components(Spec::EMPTY, Spec::new([vel_id]));
//!
//! // Multiple readers - no conflict
//! assert!(!read_pos.conflicts_with(&read_pos));
//!
//! // Reader + writer to same component - conflict
//! assert!(read_pos.conflicts_with(&write_pos));
//!
//! // Writers to different components - no conflict
//! assert!(!write_pos.conflicts_with(&write_vel));
//! ```
//!
//! # Use in Scheduler
//!
//! The scheduler uses conflict detection to determine which systems can run in parallel:
//!
//! ```rust,ignore
//! fn can_run_in_parallel(system1: &System, system2: &System) -> bool {
//!     let access1 = system1.access();
//!     let access2 = system2.access();
//!
//!     !access1.conflicts_with(&access2)
//! }
//! ```
//!
//! # Access Granting
//!
//! The [`grants`](Access::grants) method checks if one access satisfies another's requirements,
//! useful for validating that a held capability covers a requested operation:
//!
//! ```rust,ignore
//! // System has mutable Velocity access
//! let held = Access::to_components(Spec::EMPTY, Spec::new([velocity_id]));
//!
//! // Can we read Velocity? Yes (mutable grants immutable)
//! let read_vel = Access::to_components(Spec::new([velocity_id]), Spec::EMPTY);
//! assert!(held.grants(&read_vel));
//!
//! // Can we write Position? No (don't have Position access)
//! let write_pos = Access::to_components(Spec::EMPTY, Spec::new([position_id]));
//! assert!(!held.grants(&write_pos));
//! ```

use core::fmt;
use std::marker::PhantomData;

use crate::ecs::component;
use fixedbitset::FixedBitSet;

/// Bitset-based component set for fast access checking.
///
/// Uses `FixedBitSet` to track component access. Each bit represents one component ID -
/// bit N set means component ID N is included in the set.
///
/// This is an internal implementation detail of the access system, optimized for the
/// scheduler's conflict detection hot path. The bitset automatically grows to accommodate
/// any component ID, making it future-proof.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComponentSet {
    /// The bitset for tracking components.
    bitset: FixedBitSet,
    /// Cache the component length for scheduler complexity analysis.
    length: usize,
}

impl ComponentSet {
    /// Empty set with no components.
    const EMPTY: Self = Self {
        bitset: FixedBitSet::new(),
        length: 0,
    };

    /// Creates a new component set from a component specification.
    #[inline]
    fn from_spec(spec: &component::Spec) -> Self {
        let mut bitset = FixedBitSet::new();
        for id in spec.ids() {
            let index = id.index();
            bitset.grow(index + 1);
            bitset.insert(index);
        }
        Self {
            bitset,
            length: spec.ids().len(),
        }
    }

    /// Check if this set contains all components in another set.
    #[inline]
    fn contains_all(&self, other: &Self) -> bool {
        self.bitset.is_superset(&other.bitset)
    }

    /// Check if this set is empty (no components).
    #[inline]
    fn is_empty(&self) -> bool {
        self.bitset.is_clear()
    }

    /// Union of two sets (merge).
    #[inline]
    fn union(&self, other: &Self) -> Self {
        let mut union = self.bitset.clone();
        union.union_with(&other.bitset);
        // Calculate new length
        let length = union.count_ones(..);
        Self {
            bitset: union,
            length,
        }
    }

    /// Get the number of components in this set.
    #[inline]
    fn len(&self) -> usize {
        self.length
    }
}

/// A state type for access that is being requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request;
/// A state type for access that has granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant;

/// A specification of access to world resources.
///
/// `Access` represents a **capability** - what operations are permitted on the world and its
/// components. It can represent access at two levels:
///
/// 1. **World-level access** - Access to the entire world (all components)
/// 2. **Component-level access** - Access to specific components
///
/// # Access Hierarchy
///
/// Access follows a hierarchical model:
///
/// ```text
/// Mutable World Access
///   ├─> Immutable World Access
///   ├─> All Mutable Components
///   └─> All Immutable Components
///
/// Immutable World Access
///   ├─> Immutable World Access
///   └─> All Immutable Components
///
/// Component Access
///   ├─> Specified Mutable Components
///   └─> Specified Immutable Components
/// ```
///
/// # Rules
///
/// 1. **Mutable world access** grants:
///    - Mutable world access
///    - Immutable world access
///    - Any component access (mutable or immutable)
///
/// 2. **Immutable world access** grants:
///    - Immutable world access
///    - Immutable component access (any components)
///    - Does NOT grant mutable access to anything
///
/// 3. **Component access** grants:
///    - Only the specified components
///    - Mutable component access grants immutable access to same component
///    - Does NOT grant world access
///
/// # Invariants
///
/// - World access (mutable or immutable) implies no component access tracked.
///
/// # Examples
///
/// ## Creating Access
///
/// ```rust,ignore
/// use rusty_engine::ecs::world::access::Access;
/// use rusty_engine::ecs::component::Spec;
///
/// // Full mutable world access
/// let world_mut = Access::to_world(true);
///
/// // Immutable world access
/// let world = Access::to_world(false);
///
/// // Component access: read Position, write Velocity
/// let components = Access::to_components(
///     Spec::new([position_id]),
///     Spec::new([velocity_id])
/// );
///
/// // No access
/// let none = Access::NONE;
/// ```
///
/// ## Checking Access
///
/// ```rust,ignore
/// // System with mutable Velocity access
/// let system = Access::to_components(Spec::EMPTY, Spec::new([velocity_id]));
///
/// // Can read Velocity? Yes (mutable grants immutable)
/// let read = Access::to_components(Spec::new([velocity_id]), Spec::EMPTY);
/// assert!(system.grants(&read));
///
/// // Can write Position? No (different component)
/// let write = Access::to_components(Spec::EMPTY, Spec::new([position_id]));
/// assert!(!system.grants(&write));
/// ```
///
/// # Future Improvements
///
/// - Resource access tracking (`Res<T>`, `ResMut<T>`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access<State> {
    /// Access to the world itself, but immutable.
    world: bool,

    /// Access to the world itself, but mutable.
    world_mut: bool,

    /// Immutable access to a specific set of components.
    components: ComponentSet,

    /// Mutable access to a specific set of components.
    components_mut: ComponentSet,

    /// Marker for ZST state type.
    _marker: PhantomData<State>,
    // TODO: Resources..,
}

/// Type alias for [Access<Grant>] used to represent an access grant.
pub type AccessGrant = Access<Grant>;

/// Type alias for [Access<Request>] used to represent an access request.
pub type AccessRequest = Access<Request>;

impl Access<Request> {
    /// Requesting access to no wolrd resources.
    ///
    /// This is useful as a starting point when building up access requirements,
    /// or for systems that genuinely need no world access (e.g., pure computation).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let access = Access::NONE;
    /// assert!(!access.world());
    /// assert!(!access.world_mut());
    /// ```
    pub const NONE: Self = Self::new(false, false, ComponentSet::EMPTY, ComponentSet::EMPTY);

    /// Creates world-level access (immutable or mutable).
    ///
    /// World access is the highest level of access - it grants access to the entire world
    /// and all its components.
    ///
    /// # Parameters
    ///
    /// - `mutable`:
    /// - If `true`, creates mutable world access (grants everything).
    /// - If `false`, creates immutable world access (grants immutable access only).
    ///
    /// # Rules
    ///
    /// - **Mutable world access** (`mutable = true`):
    ///   - Grants mutable world access
    ///   - Grants immutable world access
    ///   - Grants any component access (mutable or immutable)
    ///
    /// - **Immutable world access** (`mutable = false`):
    ///   - Grants immutable world access
    ///   - Grants immutable component access (any components)
    ///   - Does NOT grant mutable access to anything
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // System that takes &mut World
    /// let mut_world = Access::to_world(true);
    ///
    /// // Grants any request
    /// assert!(mut_world.grants(&Access::to_world(false)));
    /// assert!(mut_world.grants(&Access::to_components(any_spec, Spec::EMPTY)));
    ///
    /// // System that takes &World
    /// let world = Access::to_world(false);
    ///
    /// // Grants immutable requests only
    /// assert!(world.grants(&Access::to_world(false)));
    /// assert!(!world.grants(&Access::to_world(true)));
    /// ```
    pub const fn to_world(mutable: bool) -> Self {
        Self::new(true, mutable, ComponentSet::EMPTY, ComponentSet::EMPTY)
    }

    /// Creates component-level access for specific components.
    ///
    /// This is fine-grained access that grants access only to the specified components,
    /// not the entire world.
    ///
    /// # Parameters
    ///
    /// - `immutable`: Components that can be read (immutable access)
    /// - `mutable`: Components that can be written (mutable access)
    ///
    /// # Rules
    ///
    /// - Mutable component access automatically grants immutable access to the same component
    /// - Component access does NOT grant world access
    /// - Only the specified components are accessible
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // System that reads Position and writes Velocity
    /// let access = Access::to_components(
    ///     Spec::new([position_id]),  // Immutable: Position
    ///     Spec::new([velocity_id])    // Mutable: Velocity
    /// );
    ///
    /// // Can read Position? Yes
    /// let read_pos = Access::to_components(Spec::new([position_id]), Spec::EMPTY);
    /// assert!(access.grants(&read_pos));
    ///
    /// // Can write Velocity? Yes
    /// let write_vel = Access::to_components(Spec::EMPTY, Spec::new([velocity_id]));
    /// assert!(access.grants(&write_vel));
    ///
    /// // Can read Velocity? Yes (mutable grants immutable)
    /// let read_vel = Access::to_components(Spec::new([velocity_id]), Spec::EMPTY);
    /// assert!(access.grants(&read_vel));
    ///
    /// // Can write Position? No (only have read access)
    /// let write_pos = Access::to_components(Spec::EMPTY, Spec::new([position_id]));
    /// assert!(!access.grants(&write_pos));
    /// ```
    #[inline]
    pub fn to_components(immutable: component::Spec, mutable: component::Spec) -> Self {
        let immutable_set = ComponentSet::from_spec(&immutable);
        let mutable_set = ComponentSet::from_spec(&mutable);
        Self::new(false, false, immutable_set.union(&mutable_set), mutable_set)
    }

    /// Convert an access request into an access grant.
    #[inline]
    fn as_grant(&self) -> AccessGrant {
        AccessGrant::new(
            self.world,
            self.world_mut,
            self.components.clone(),
            self.components_mut.clone(),
        )
    }
}

impl Access<Grant> {
    /// Determines if this access grants the requested access.
    ///
    /// Returns `true` if the capabilities represented by `self` are sufficient to satisfy
    /// the access `request`. This is the core method for validating permissions.
    ///
    /// # Parameters
    ///
    /// - `request`: The access being requested
    ///
    /// # Returns
    ///
    /// `true` if this access grants the request, `false` otherwise.
    ///
    /// # Grant Rules
    ///
    /// 1. **Mutable world access** grants everything
    /// 2. **Immutable world access** grants:
    ///    - Immutable world access
    ///    - Any immutable component access
    /// 3. **Component access** grants:
    ///    - Requested components if they're in the spec
    ///    - Immutable access if we have mutable access to the component
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use rusty_engine::ecs::world::access::Access;
    /// use rusty_engine::ecs::component::Spec;
    ///
    /// // Mutable world grants everything
    /// let world_mut = Access::to_world(true);
    /// assert!(world_mut.grants(&Access::to_world(false)));
    /// assert!(world_mut.grants(&Access::to_components(any_spec, Spec::EMPTY)));
    ///
    /// // Mutable component grants immutable to same component
    /// let mut_vel = Access::to_components(Spec::EMPTY, Spec::new([velocity_id]));
    /// let read_vel = Access::to_components(Spec::new([velocity_id]), Spec::EMPTY);
    /// assert!(mut_vel.grants(&read_vel));
    ///
    /// // But not to different components
    /// let read_pos = Access::to_components(Spec::new([position_id]), Spec::EMPTY);
    /// assert!(!mut_vel.grants(&read_pos));
    /// ```
    ///
    /// # Note
    ///
    /// For scheduler conflict detection, use [`Access::conflicts_with`] instead.
    /// The `grants` method is for validating that held access covers a requested operation.
    pub fn grants(&self, request: &AccessRequest) -> bool {
        // Start by comparing world access.
        match (self.world_mut, self.world, request.world_mut, request.world) {
            // We are mutable world access, so we grant everything.
            (true, _, _, _) => true,
            // Immutable world access, but world mutable requested, so deny.
            (false, true, true, _) => false,
            // Immutable world access, and world immutable requested, so grant.
            (false, true, false, true) => true,
            // We are immutable world access, so we grant only immutable component access requests.
            (false, true, false, false) => request.components_mut.is_empty(),
            // We do not have world access, so deny world request.
            (false, false, true, _) => false,
            (false, false, _, true) => false,
            // Neither have world access, so check component access.
            (false, false, false, false) => {
                if self.components.contains_all(&request.components)
                    && self.components_mut.contains_all(&request.components_mut)
                {
                    // Both component access granted.
                    true
                } else {
                    //Cannot grant component access.
                    false
                }
            }
        }
    }
}

impl<State> Access<State> {
    /// Private constructor for creating an `Access` instances.
    #[inline]
    const fn new(
        world: bool,
        world_mut: bool,
        components: ComponentSet,
        components_mut: ComponentSet,
    ) -> Self {
        Self {
            world,
            world_mut,
            components,
            components_mut,
            _marker: PhantomData,
        }
    }

    /// Returns `true` if this access includes immutable world access.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let access = Access::to_world(false);
    /// assert!(access.world());
    /// assert!(!access.world_mut());
    /// ```
    #[inline]
    pub const fn world(&self) -> bool {
        self.world
    }

    /// Returns `true` if this access includes mutable world access.
    ///
    /// Note: Mutable world access implies immutable world access as well.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let access = Access::to_world(true);
    /// assert!(access.world());      // true - also has immutable
    /// assert!(access.world_mut());  // true - has mutable
    /// ```
    #[inline]
    pub const fn world_mut(&self) -> bool {
        self.world_mut
    }

    /// Get the length of immutable component access.
    #[inline]
    pub fn components_len(&self) -> usize {
        self.components.len()
    }

    /// Get the length of mutable component access.
    #[inline]
    pub fn components_mut_len(&self) -> usize {
        self.components_mut.len()
    }

    /// Returns `true` if this access conflicts with another access.
    ///
    /// Two accesses conflict if they cannot be held simultaneously according to
    /// Rust's aliasing rules:
    /// - Multiple immutable accesses to the same resource are OK
    /// - Mutable access conflicts with any other access (mutable or immutable) to the same resource
    ///
    /// # Rules
    ///
    /// 1. **Mutable world access** conflicts with any non-empty access
    /// 2. **Immutable world access** conflicts with any mutable access (world or component)
    /// 3. **Component access** conflicts if:
    ///    - Either side has mutable access to a component the other touches
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use rusty_engine::ecs::world::access::Access;
    /// use rusty_engine::ecs::component::Spec;
    ///
    /// let read_pos = Access::to_components(Spec::new([pos_id]), Spec::EMPTY);
    /// let write_pos = Access::to_components(Spec::EMPTY, Spec::new([pos_id]));
    /// let read_vel = Access::to_components(Spec::new([vel_id]), Spec::EMPTY);
    ///
    /// // Multiple readers - no conflict
    /// assert!(!read_pos.conflicts_with(&read_pos));
    ///
    /// // Reader + writer - conflict
    /// assert!(read_pos.conflicts_with(&write_pos));
    ///
    /// // Different components - no conflict
    /// assert!(!read_pos.conflicts_with(&read_vel));
    /// ```
    pub fn conflicts_with<Other>(&self, other: &Access<Other>) -> bool {
        // If either is empty, no conflict
        if self.is_none() || other.is_none() {
            return false;
        }

        // Mutable world access conflicts with any non-empty access
        if self.world_mut || other.world_mut {
            return true;
        }

        // Immutable world access conflicts with any mutable access
        if self.world && !other.components_mut.is_empty() {
            return true;
        }
        if other.world && !self.components_mut.is_empty() {
            return true;
        }

        // Component-level conflicts: mutable access to a component conflicts with
        // any access (mutable or immutable) to the same component
        if !self
            .components_mut
            .bitset
            .is_disjoint(&other.components.bitset)
        {
            return true;
        }
        if !other
            .components_mut
            .bitset
            .is_disjoint(&self.components.bitset)
        {
            return true;
        }

        false
    }

    /// Returns `true` if this access is nothing (no access to anything).
    #[inline]
    pub fn is_none(&self) -> bool {
        !self.world
            && !self.world_mut
            && self.components.is_empty()
            && self.components_mut.is_empty()
    }

    /// Merges this access with another, returning a new access that combines both.
    #[inline]
    pub fn merge(&self, other: &Access<State>) -> Access<State> {
        // Determine if merge results in world access.
        let world_mut = self.world_mut || other.world_mut;
        let world = self.world || other.world || world_mut; // Ensure mutable implies immutable.

        // If world access is granted, no components, otherwise merge component access as well.
        let (components, components_mut) = match world {
            true => (ComponentSet::EMPTY, ComponentSet::EMPTY),
            false => (
                self.components.union(&other.components),
                self.components_mut.union(&other.components_mut),
            ),
        };
        Access {
            world,
            world_mut,
            components,
            components_mut,
            _marker: PhantomData,
        }
    }
}

/// Utility for tracking active access grants in a world. The intent is to maintain any active
/// grants for the lifetime of a shards or other access-controlled contexts. Once a context is
/// dropped, the grant should be returned to the world via this tracker.
///
///
/// Note: that this is a simple implementation and may not be optimal for large numbers of grants.
/// This might be fine for typical ECS usage where the number of concurrent grants is small.
pub(crate) struct GrantTracker {
    active: Vec<AccessGrant>,
}

impl GrantTracker {
    /// Create a new, empty grant tracker.
    #[inline]
    pub const fn new() -> Self {
        Self { active: Vec::new() }
    }

    /// Determine whether the given request is valid (does not conflict with any active grants).
    #[inline]
    pub fn is_valid(&self, request: &AccessRequest) -> bool {
        for active in &self.active {
            if request.conflicts_with(active) {
                return false;
            }
        }
        true
    }

    /// Check if the request conflicts with any active grants, and if not, add it to the active
    /// set.
    pub fn check_and_grant(
        &mut self,
        request: &AccessRequest,
    ) -> Result<AccessGrant, ConflictError> {
        if self.is_valid(request) {
            let grant = request.as_grant();
            self.active.push(grant.clone());
            Ok(grant)
        } else {
            Err(ConflictError::new(request.clone()))
        }
    }

    /// Remove an active grant matching the given request.
    pub fn remove(&mut self, grant: &AccessGrant) {
        // Remove matching grant
        if let Some(pos) = self.active.iter().position(|g| g == grant) {
            self.active.swap_remove(pos);
        }
    }
}

/// An error indicating a conflict in an access request with the existing grants.
#[derive(Debug, Clone)]
pub struct ConflictError {
    /// The conflicting access request.
    request: AccessRequest,
}

impl ConflictError {
    /// Constructs a new `ConflictError` for the given access request.
    #[inline]
    pub const fn new(request: AccessRequest) -> Self {
        Self { request }
    }
}

impl fmt::Display for ConflictError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid access request: {:?}", self.request)
    }
}

#[cfg(test)]
mod tests {
    use crate::ecs::{
        component,
        world::access::{Access, GrantTracker},
    };

    #[test]
    fn test_world_access() {
        // Given
        let grant = super::Access::to_world(true).as_grant();
        let request = super::Access::to_world(true);

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);

        // Given
        let grant = super::Access::to_world(true).as_grant();
        let request = super::Access::to_world(false);

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);

        // Given
        let grant = super::Access::to_world(false).as_grant();
        let request = super::Access::to_world(true);

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(!grants);

        // Given
        let grant = super::Access::to_world(false).as_grant();
        let request = super::Access::to_world(false);

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);
    }

    #[test]
    pub fn world_and_component_access() {
        // Given - mutable world access should grant all immutable component access
        let grant = super::Access::to_world(true).as_grant();
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);

        // Given - mutable world access should grant all mutable component access
        let grant = super::Access::to_world(true).as_grant();
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);

        // Given - immutable world access should grant all immutable component access
        let grant = super::Access::to_world(false).as_grant();
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(grants);

        // Given - immutable world access should *not* grant all mutable component access
        let grant = super::Access::to_world(false).as_grant();
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // When
        let grants = grant.grants(&request);

        // Then
        assert!(!grants);
    }

    #[test]
    fn mutable_component_to_mutable_component() {
        // Given - access to mutable Position
        let grant = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        )
        .as_grant();

        // When - requesting mutable Position
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // Then - should grant
        assert!(grant.grants(&request));
    }

    #[test]
    fn mutable_component_to_immutable_component() {
        // Given - access to mutable Position
        let grant = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        )
        .as_grant();

        // When - requesting immutable Position
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        // Then - should grant (mutable implies immutable)
        assert!(grant.grants(&request));
    }

    #[test]
    fn immutable_component_to_immutable_component() {
        // Given - access to immutable Position
        let grant = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )
        .as_grant();

        // When - requesting immutable Position
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        // Then - should grant
        assert!(grant.grants(&request));
    }

    #[test]
    fn immutable_component_to_mutable_component() {
        // Given - access to immutable Position
        let grant = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )
        .as_grant();

        // When - requesting mutable Position
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // Then - should NOT grant (immutable doesn't grant mutable)
        assert!(!grant.grants(&request));
    }

    #[test]
    fn multiple_components_subset() {
        // Given - access to Position and Velocity (both mutable)
        let grant = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0), component::Id::new(1)]),
        )
        .as_grant();

        // When - requesting just Position (mutable)
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // Then - should grant (subset)
        assert!(grant.grants(&request));
    }

    #[test]
    fn multiple_components_superset() {
        // Given - access to just Position (mutable)
        let grant = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        )
        .as_grant();

        // When - requesting Position and Velocity (mutable)
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0), component::Id::new(1)]),
        );

        // Then - should NOT grant (doesn't have Velocity)
        assert!(!grant.grants(&request));
    }

    #[test]
    fn disjoint_components() {
        // Given - access to Position (mutable)
        let grant = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        )
        .as_grant();

        // When - requesting Velocity (mutable)
        let request = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(1)]),
        );

        // Then - should NOT grant (completely different component)
        assert!(!grant.grants(&request));
    }

    #[test]
    fn mixed_mutability_components() {
        // Given - access to Position (immutable) and Velocity (mutable)
        let grant = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        )
        .as_grant();

        // When - requesting Position (immutable) and Velocity (immutable)
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0), component::Id::new(1)]),
            component::Spec::EMPTY,
        );

        // Then - should grant (mutable Velocity grants immutable)
        assert!(grant.grants(&request));
    }

    #[test]
    fn no_access_grants_nothing() {
        // Given - no access at all
        let grant = super::Access::NONE.as_grant();

        // When - requesting any component
        let request = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        // Then - should NOT grant
        assert!(!grant.grants(&request));
    }

    #[test]
    fn component_access_denies_world_access() {
        // Given - access to some components
        let grant = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )
        .as_grant();

        // When - requesting world access
        let request = super::Access::to_world(false);

        // Then - should NOT grant
        assert!(!grant.grants(&request));
    }

    // ==================== conflicts_with tests ====================

    #[test]
    fn no_access_conflicts_with_nothing() {
        let none = super::Access::NONE;
        let world_mut = super::Access::to_world(true);
        let world = super::Access::to_world(false);
        let component = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        assert!(!none.conflicts_with(&none));
        assert!(!none.conflicts_with(&world_mut));
        assert!(!none.conflicts_with(&world));
        assert!(!none.conflicts_with(&component));
    }

    #[test]
    fn mutable_world_conflicts_with_everything() {
        let world_mut = super::Access::to_world(true);
        let world = super::Access::to_world(false);
        let read_comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let write_comp = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        assert!(world_mut.conflicts_with(&world_mut));
        assert!(world_mut.conflicts_with(&world));
        assert!(world_mut.conflicts_with(&read_comp));
        assert!(world_mut.conflicts_with(&write_comp));
    }

    #[test]
    fn immutable_world_no_conflict_with_immutable_world() {
        let world1 = super::Access::to_world(false);
        let world2 = super::Access::to_world(false);

        assert!(!world1.conflicts_with(&world2));
    }

    #[test]
    fn immutable_world_conflicts_with_mutable_components() {
        let world = super::Access::to_world(false);
        let write_comp = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        assert!(world.conflicts_with(&write_comp));
        assert!(write_comp.conflicts_with(&world));
    }

    #[test]
    fn immutable_world_no_conflict_with_immutable_components() {
        let world = super::Access::to_world(false);
        let read_comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        assert!(!world.conflicts_with(&read_comp));
    }

    #[test]
    fn multiple_immutable_same_component_no_conflict() {
        let read1 = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let read2 = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        assert!(!read1.conflicts_with(&read2));
    }

    #[test]
    fn immutable_and_mutable_same_component_conflict() {
        let read = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let write = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        assert!(read.conflicts_with(&write));
        assert!(write.conflicts_with(&read));
    }

    #[test]
    fn mutable_and_mutable_same_component_conflict() {
        let write1 = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );
        let write2 = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        assert!(write1.conflicts_with(&write2));
    }

    #[test]
    fn different_components_no_conflict() {
        let pos_read = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let vel_write = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(1)]),
        );

        assert!(!pos_read.conflicts_with(&vel_write));
    }

    #[test]
    fn overlapping_components_conflict() {
        // A reads Position, writes Velocity
        let a = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );
        // B reads Velocity (conflicts with A's write to Velocity)
        let b = super::Access::to_components(
            component::Spec::new([component::Id::new(1)]),
            component::Spec::EMPTY,
        );

        assert!(a.conflicts_with(&b));
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn non_overlapping_mixed_access_no_conflict() {
        // A reads Position, writes Velocity
        let a = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );
        // B reads Acceleration, writes Health
        let b = super::Access::to_components(
            component::Spec::new([component::Id::new(2)]),
            component::Spec::new([component::Id::new(3)]),
        );

        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn is_empty() {
        assert!(super::Access::NONE.is_none());
        assert!(!super::Access::to_world(false).is_none());
        assert!(!super::Access::to_world(true).is_none());
        assert!(
            !super::Access::to_components(
                component::Spec::new([component::Id::new(0)]),
                component::Spec::EMPTY,
            )
            .is_none()
        );
    }

    // ==================== merge tests ====================

    #[test]
    fn merge_none_with_none() {
        let result = super::Access::NONE.merge(&super::Access::NONE);

        assert!(result.is_none());
    }

    #[test]
    fn merge_none_with_component_access() {
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );

        let result = super::Access::NONE.merge(&comp).as_grant();

        // Should have the component access
        assert!(!result.world());
        assert!(!result.world_mut());
        // Verify grants work as expected
        assert!(result.grants(&super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )));
        assert!(result.grants(&super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(1)]),
        )));
    }

    #[test]
    fn merge_component_with_none() {
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );

        let result = comp.merge(&super::Access::NONE).as_grant();

        assert!(!result.world());
        assert!(result.grants(&super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )));
    }

    #[test]
    fn merge_component_with_component_unions() {
        // A: reads Position
        let a = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        // B: writes Velocity
        let b = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(1)]),
        );

        let result = a.merge(&b).as_grant();

        // Should have both accesses
        assert!(!result.world());
        assert!(result.grants(&super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        )));
        assert!(result.grants(&super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(1)]),
        )));
        // Combined request should also work
        assert!(result.grants(&super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        )));
    }

    #[test]
    fn merge_component_with_world_clears_components() {
        // Component access with Position read
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let world = super::Access::to_world(false);

        let result = comp.merge(&world);

        // Should be world access with no tracked components
        assert!(result.world());
        assert!(!result.world_mut());
        // Invariant: world access means components are empty
        // This is verified by checking equality with to_world(false)
        assert_eq!(result, super::Access::to_world(false));
    }

    #[test]
    fn merge_world_with_component_clears_components() {
        let world = super::Access::to_world(false);
        let comp = super::Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        let result = world.merge(&comp);

        // Should be world access, components cleared
        assert!(result.world());
        assert!(!result.world_mut());
        assert_eq!(result, super::Access::to_world(false));
    }

    #[test]
    fn merge_world_mut_with_component_clears_components() {
        let world_mut = super::Access::to_world(true);
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );

        let result = world_mut.merge(&comp);

        // Should be mutable world access, components cleared
        assert!(result.world());
        assert!(result.world_mut());
        assert_eq!(result, super::Access::to_world(true));
    }

    #[test]
    fn merge_component_with_world_mut_clears_components() {
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );
        let world_mut = super::Access::to_world(true);

        let result = comp.merge(&world_mut);

        assert!(result.world());
        assert!(result.world_mut());
        assert_eq!(result, super::Access::to_world(true));
    }

    #[test]
    fn merge_immutable_world_with_immutable_world() {
        let world1 = super::Access::to_world(false);
        let world2 = super::Access::to_world(false);

        let result = world1.merge(&world2);

        assert!(result.world());
        assert!(!result.world_mut());
        assert_eq!(result, super::Access::to_world(false));
    }

    #[test]
    fn merge_immutable_world_with_mutable_world() {
        let world = super::Access::to_world(false);
        let world_mut = super::Access::to_world(true);

        let result = world.merge(&world_mut);

        // Mutable wins
        assert!(result.world());
        assert!(result.world_mut());
        assert_eq!(result, super::Access::to_world(true));
    }

    #[test]
    fn merge_mutable_world_with_immutable_world() {
        let world_mut = super::Access::to_world(true);
        let world = super::Access::to_world(false);

        let result = world_mut.merge(&world);

        assert!(result.world());
        assert!(result.world_mut());
        assert_eq!(result, super::Access::to_world(true));
    }

    #[test]
    fn merge_mutable_world_with_mutable_world() {
        let world_mut1 = super::Access::to_world(true);
        let world_mut2 = super::Access::to_world(true);

        let result = world_mut1.merge(&world_mut2);

        assert!(result.world());
        assert!(result.world_mut());
        assert_eq!(result, super::Access::to_world(true));
    }

    #[test]
    fn merge_is_commutative_for_components() {
        let a = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::new([component::Id::new(1)]),
        );
        let b = super::Access::to_components(
            component::Spec::new([component::Id::new(2)]),
            component::Spec::new([component::Id::new(3)]),
        );

        let ab = a.merge(&b);
        let ba = b.merge(&a);

        assert_eq!(ab, ba);
    }

    #[test]
    fn merge_is_commutative_for_world() {
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let world = super::Access::to_world(false);

        let cw = comp.merge(&world);
        let wc = world.merge(&comp);

        assert_eq!(cw, wc);
    }

    #[test]
    fn merge_preserves_invariant_components_empty_when_world() {
        // Start with component access
        let comp = super::Access::to_components(
            component::Spec::new([component::Id::new(0), component::Id::new(1)]),
            component::Spec::new([component::Id::new(2), component::Id::new(3)]),
        );
        let world = super::Access::to_world(false);

        let result = comp.merge(&world);

        // The invariant: if world access, components must be empty
        // We verify this by checking that the result doesn't grant
        // component requests that it "shouldn't" if it were world access
        // (world access grants all immutable component requests)
        assert!(result.world());

        // Also verify against a freshly created world access
        let fresh_world = super::Access::to_world(false);
        assert_eq!(result, fresh_world);
    }

    #[test]
    fn grant_tracker_conflict_detection() {
        let mut tracker = GrantTracker { active: vec![] };

        let read_pos = Access::to_components(
            component::Spec::new([component::Id::new(0)]),
            component::Spec::EMPTY,
        );
        let write_pos = Access::to_components(
            component::Spec::EMPTY,
            component::Spec::new([component::Id::new(0)]),
        );

        // Check and issue the grant.
        let grant = tracker.check_and_grant(&read_pos);

        // Add read access - should succeed
        assert!(grant.is_ok());

        let grant = grant.unwrap();

        // Add write access - should conflict
        assert!(tracker.check_and_grant(&write_pos).is_err());

        // Remove read access
        tracker.remove(&grant);

        // Now adding write access should succeed
        assert!(tracker.check_and_grant(&write_pos.clone()).is_ok());
    }
}
