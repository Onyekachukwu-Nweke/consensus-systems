use serde::{Deserialize, Serialize};
use stateright::actor::*;
use std::collections::HashMap;
use std::hash::Hash;

/// Node ID type
pub type NodeId = u8;

/// Possible values for consensus
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Value {
    V1,
    V2,
}

/// Node states in the consensus protocol
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    Init,
    Prepared,
    Committed,
    Decided,
    Failed,
}

/// Message types in the protocol
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum MessageType {
    Propose(Value),
    Prepare(Value),
    Commit(Value),
    Decide(Value),
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
        }
    }

    pub fn has_quorum(&self, count: usize) -> bool {
        count >= self.quorum_size
    }
}

/// Actor implementing consensus protocol
pub struct ConsensusActor {
    pub peers: Vec<Id>,
}

impl ConsensusActor {
    pub fn new(peers: Vec<Id>) -> Self {
        ConsensusActor { peers }
    }
}

impl Actor for ConsensusActor {
    type Msg = MessageType;
    type State = ConsensusNodeState;
    type Timer = ();

    fn on_start(&self, id: Id, o: &mut Out<Self>) -> Self::State {
        let node_id = id.into();
        // Quorum = 2f + 1, with f=2 for 5 nodes -> quorum = 5
        ConsensusNodeState::new(node_id, 5)
    }

    fn on_msg(
        &self,
        _id: Id,
        state: &mut Cow<Self::State>,
        src: Id,
        msg: Self::Msg,
        o: &mut Out<Self>,
    ) {
        // Don't process messages if node is faulty
        if state.is_faulty {
            return;
        }

        match msg {
            MessageType::Propose(value) => {
                // Receive proposal
                if state.state == NodeState::Init && state.value.is_none() {
                    let mut new_state = state.as_ref().clone();
                    new_state.value = Some(value.clone());
                    
                    // Broadcast PREPARE
                    for &peer in &self.peers {
                        o.send(peer, MessageType::Prepare(value.clone()));
                    }
                    
                    *state = Cow::Owned(new_state);
                }
            }

            MessageType::Prepare(value) => {
                // Count PREPARE messages
                if let Some(ref my_value) = state.value {
                    if *my_value == value {
                        let mut new_state = state.as_ref().clone();
                        let count = new_state.prepare_count.entry(value.clone()).or_insert(0);
                        *count += 1;

                        // If we have quorum of PREPAREs, move to PREPARED and broadcast COMMIT
                        if new_state.has_quorum(*count) && new_state.state == NodeState::Init {
                            new_state.state = NodeState::Prepared;
                            
                            for &peer in &self.peers {
                                o.send(peer, MessageType::Commit(value.clone()));
                            }
                        }

                        *state = Cow::Owned(new_state);
                    }
                }
            }

            MessageType::Commit(value) => {
                // Count COMMIT messages
                if state.state == NodeState::Prepared {
                    if let Some(ref my_value) = state.value {
                        if *my_value == value {
                            let mut new_state = state.as_ref().clone();
                            let count = new_state.commit_count.entry(value.clone()).or_insert(0);
                            *count += 1;

                            // If we have quorum of COMMITs, move to COMMITTED and broadcast DECIDE
                            if new_state.has_quorum(*count) {
                                new_state.state = NodeState::Committed;
                                
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
                // Finalize decision
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
}

/// Model configuration for testing
pub struct ConsensusModel {
    pub num_nodes: usize,
    pub max_faults: usize,
}

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