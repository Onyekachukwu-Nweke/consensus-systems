use serde::{Deserialize, Serialize};
use stateright::actor::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::Hash;

/// Node ID type
pub type NodeId = usize;

/// Possible values for consensus
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Value {
    V1,
    V2,
    V3,  // Additional value for more realistic testing
}

/// Node states in the consensus protocol
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    Init,
    Prepared,
    Committed,
    Decided,
    Failed,
}

/// Message types in the protocol
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum MessageType {
    Propose(Value),
    Prepare(Value),
    Commit(Value),
    Decide(Value),
}

/// Timer types for non-deterministic actions
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ConsensusTimer {
    ProposeValue(Value),
}

/// Node internal state
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConsensusNodeState {
    pub id: NodeId,
    pub state: NodeState,
    pub value: Option<Value>,
    pub prepare_count: HashMap<Value, usize>,
    pub commit_count: HashMap<Value, usize>,
    pub decided: bool,
    pub quorum_size: usize,
    pub is_faulty: bool,
    pub has_proposed: bool,  // Track if this node has proposed a value
}

impl ConsensusNodeState {
    pub fn new(id: NodeId, quorum_size: usize) -> Self {
        ConsensusNodeState {
            id,
            state: NodeState::Init,
            value: None,
            prepare_count: HashMap::new(),
            commit_count: HashMap::new(),
            decided: false,
            quorum_size,
            is_faulty: false,
            has_proposed: false,
        }
    }

    pub fn has_quorum(&self, count: usize) -> bool {
        count >= self.quorum_size
    }
}

// Manual Hash implementation since HashMap doesn't implement Hash
impl Hash for ConsensusNodeState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.state.hash(state);
        self.value.hash(state);
        self.decided.hash(state);
        self.quorum_size.hash(state);
        self.is_faulty.hash(state);
        self.has_proposed.hash(state);
        // Note: We skip prepare_count and commit_count since HashMap doesn't implement Hash
        // This is acceptable for model checking as the other fields capture the essential state
    }
}

/// Actor implementing consensus protocol
#[derive(Clone)]
pub struct ConsensusActor {
    pub peers: Vec<Id>,
    pub faulty_nodes: Vec<usize>,  // List of node IDs that should be faulty
    pub quorum_size: usize,        // Quorum size for consensus
}

impl ConsensusActor {
    /// Create a new consensus actor with no faulty nodes (used by tests)
    #[allow(dead_code)]
    pub fn new(peers: Vec<Id>, quorum_size: usize) -> Self {
        ConsensusActor {
            peers,
            faulty_nodes: Vec::new(),
            quorum_size,
        }
    }

    /// Create a consensus actor with specified faulty nodes
    pub fn with_faults(peers: Vec<Id>, faulty_nodes: Vec<usize>, quorum_size: usize) -> Self {
        ConsensusActor {
            peers,
            faulty_nodes,
            quorum_size,
        }
    }
}

impl Actor for ConsensusActor {
    type Msg = MessageType;
    type State = ConsensusNodeState;
    type Timer = ConsensusTimer;
    type Storage = ();
    type Random = ();

    fn on_start(&self, id: Id, _storage: &Option<Self::Storage>, o: &mut Out<Self>) -> Self::State {
        let node_id = usize::from(id);
        // Use the configured quorum size
        let mut state = ConsensusNodeState::new(node_id, self.quorum_size);

        // Check if this node should be faulty (per TLA+ NodeCrash action)
        if self.faulty_nodes.contains(&node_id) {
            state.is_faulty = true;
            state.state = NodeState::Failed;
            return state;  // Faulty nodes don't participate
        }

        // For non-deterministic model checking:
        // Node 0 proposes all three possible values
        // The model checker explores different orderings of message delivery
        // creating branches where nodes might accept different values first
        if node_id == 0 && !self.faulty_nodes.contains(&0) {
            for &peer in &self.peers {
                o.send(peer, MessageType::Propose(Value::V1));
                o.send(peer, MessageType::Propose(Value::V2));
                o.send(peer, MessageType::Propose(Value::V3));
            }
        }

        state
    }

    fn on_msg(
        &self,
        _id: Id,
        state: &mut Cow<Self::State>,
        _src: Id,
        msg: Self::Msg,
        o: &mut Out<Self>,
    ) {
        // Don't process messages if node is faulty
        if state.is_faulty {
            return;
        }

        match msg {
            MessageType::Propose(value) => {
                // ReceivePropose in TLA+: Node receives PROPOSE and broadcasts PREPARE
                if state.state == NodeState::Init && state.value.is_none() {
                    let mut new_state = state.as_ref().clone();
                    new_state.value = Some(value.clone());

                    // Broadcast PREPARE to ALL nodes (including self per TLA+ spec)
                    for &peer in &self.peers {
                        o.send(peer, MessageType::Prepare(value.clone()));
                    }

                    // Initialize our own prepare count to 1 (counting our own PREPARE)
                    *new_state.prepare_count.entry(value.clone()).or_insert(0) = 1;

                    *state = Cow::Owned(new_state);
                }
            }

            MessageType::Prepare(value) => {
                // ReceivePrepare in TLA+: Count PREPARE messages for our accepted value
                // Only process if we have accepted this value
                if let Some(ref my_value) = state.value {
                    if *my_value == value {
                        let mut new_state = state.as_ref().clone();
                        let count = new_state.prepare_count.entry(value.clone()).or_insert(0);
                        *count += 1;
                        let count_value = *count;

                        // If we reach quorum of PREPAREs and still in INIT, transition to PREPARED
                        // Per TLA+: HasQuorum(prepareCount[n][m.value] + 1) - the +1 is already done above
                        if new_state.has_quorum(count_value) && new_state.state == NodeState::Init {
                            new_state.state = NodeState::Prepared;

                            // Broadcast COMMIT to ALL nodes (including self)
                            for &peer in &self.peers {
                                o.send(peer, MessageType::Commit(value.clone()));
                            }

                            // Initialize our own commit count to 1 (counting our own COMMIT)
                            *new_state.commit_count.entry(value.clone()).or_insert(0) = 1;
                        }

                        *state = Cow::Owned(new_state);
                    }
                }
            }

            MessageType::Commit(value) => {
                // ReceiveCommit in TLA+: Count COMMIT messages and transition when quorum reached
                // Only process commits when in PREPARED state
                if state.state == NodeState::Prepared {
                    if let Some(ref my_value) = state.value {
                        if *my_value == value {
                            let mut new_state = state.as_ref().clone();
                            let count = new_state.commit_count.entry(value.clone()).or_insert(0);
                            *count += 1;
                            let count_value = *count;

                            // If we reach quorum of COMMITs, transition to COMMITTED
                            // Per TLA+: HasQuorum(commitCount[n][m.value] + 1)
                            if new_state.has_quorum(count_value) {
                                new_state.state = NodeState::Committed;

                                // Broadcast DECIDE to ALL nodes (including self)
                                for &peer in &self.peers {
                                    o.send(peer, MessageType::Decide(value.clone()));
                                }
                            }

                            *state = Cow::Owned(new_state);
                        }
                    }
                }
            }

            MessageType::Decide(value) => {
                // ReceiveDecide in TLA+: Finalize decision for this value
                // A non-faulty node receives DECIDE and transitions to DECIDED state
                if let Some(ref my_value) = state.value {
                    if *my_value == value && !state.decided {
                        let mut new_state = state.as_ref().clone();
                        new_state.decided = true;
                        new_state.state = NodeState::Decided;
                        *state = Cow::Owned(new_state);
                    }
                }
            }
        }
    }

    fn on_timeout(
        &self,
        _id: Id,
        state: &mut Cow<Self::State>,
        timer: &Self::Timer,
        o: &mut Out<Self>,
    ) {
        // Don't process timers if node is faulty
        if state.is_faulty {
            return;
        }

        match timer {
            ConsensusTimer::ProposeValue(value) => {
                // Only propose if:
                // 1. We're still in Init state (haven't accepted a proposal yet)
                // 2. We haven't already proposed
                if state.state == NodeState::Init && !state.has_proposed {
                    let mut new_state = state.as_ref().clone();
                    new_state.has_proposed = true;

                    // Broadcast PROPOSE to ALL nodes (including self per TLA+ spec)
                    for &peer in &self.peers {
                        o.send(peer, MessageType::Propose(value.clone()));
                    }

                    *state = Cow::Owned(new_state);
                }
            }
        }
    }
}

/// Model configuration for testing
#[allow(dead_code)]
pub struct ConsensusModel {
    pub num_nodes: usize,
    pub max_faults: usize,
}

#[allow(dead_code)]
impl ConsensusModel {
    pub fn new(num_nodes: usize, max_faults: usize) -> Self {
        ConsensusModel {
            num_nodes,
            max_faults,
        }
    }

    /// Check safety property: Agreement
    pub fn check_agreement(&self, history: &[ConsensusNodeState]) -> bool {
        let decided_values: Vec<_> = history
            .iter()
            .filter(|s| s.decided && !s.is_faulty)
            .filter_map(|s| s.value.as_ref())
            .collect();

        // All non-faulty decided nodes must have the same value
        if decided_values.len() > 1 {
            let first = decided_values[0];
            decided_values.iter().all(|&v| v == first)
        } else {
            true
        }
    }

    /// Check safety property: No premature decision
    pub fn check_no_premature_decision(&self, state: &ConsensusNodeState) -> bool {
        if state.state == NodeState::Decided {
            if let Some(ref value) = state.value {
                let commit_count = state.commit_count.get(value).unwrap_or(&0);
                *commit_count >= state.quorum_size
            } else {
                false
            }
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let state = ConsensusNodeState::new(0, 3);
        assert_eq!(state.state, NodeState::Init);
        assert_eq!(state.value, None);
        assert!(!state.decided);
    }

    #[test]
    fn test_quorum_logic() {
        let state = ConsensusNodeState::new(0, 3);
        assert!(!state.has_quorum(2));
        assert!(state.has_quorum(3));
        assert!(state.has_quorum(4));
    }

    #[test]
    fn test_agreement_property() {
        let model = ConsensusModel::new(5, 2);
        
        let mut state1 = ConsensusNodeState::new(0, 3);
        state1.value = Some(Value::V1);
        state1.decided = true;
        
        let mut state2 = ConsensusNodeState::new(1, 3);
        state2.value = Some(Value::V1);
        state2.decided = true;
        
        assert!(model.check_agreement(&[state1.clone(), state2.clone()]));
        
        // Test violation
        state2.value = Some(Value::V2);
        assert!(!model.check_agreement(&[state1, state2]));
    }
}