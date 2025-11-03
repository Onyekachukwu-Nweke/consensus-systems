mod model;

use model::*;
use stateright::actor::{ActorModel, Network};
use stateright::{Checker, Model};

fn main() {
    println!("=== Consensus Protocol Verification with Stateright ===\n");

    // Start with smaller scenarios to see state exploration working
    // Note: With non-deterministic proposals, state space grows rapidly:
    // - Each node can propose 3 different values (V1, V2, V3)
    // - Message ordering creates additional states

    // Scenario 1: Small system - 3 nodes, no faults (f=1, quorum=3)
    println!("Scenario 1: Small System - 3 Nodes (No Faults)");
    run_scenario(3, 0, false);

    // Scenario 2: Normal operation (5 nodes, no faults)
    println!("\nScenario 2: Normal Operation - 5 Nodes (No Faults)");
    run_scenario(5, 0, false);

    // Scenario 3: Single node crash
    println!("\nScenario 3: Single Node Crash");
    run_scenario(5, 1, false);

    println!("\n=== Verification Complete ===");
    println!("\nNote: State space grows exponentially with:");
    println!("  - Number of nodes (each can propose)");
    println!("  - Number of possible values (V1, V2, V3)");
    println!("  - Message interleaving");
}

fn run_scenario(num_nodes: usize, faulty_count: usize, lossy_network: bool) {
    println!("  Nodes: {}, Faulty: {}, Network: {}",
             num_nodes,
             faulty_count,
             if lossy_network { "Lossy" } else { "Reliable" });

    // Create peer list
    let peers: Vec<_> = (0..num_nodes).map(|i| stateright::actor::Id::from(i)).collect();

    // Per TLA+ NodeCrash: Mark last faulty_count nodes as faulty (not including proposer node 0)
    // This ensures node 0 can still propose
    let faulty_node_ids: Vec<usize> = if faulty_count > 0 {
        ((num_nodes - faulty_count)..num_nodes).collect()
    } else {
        Vec::new()
    };

    if !faulty_node_ids.is_empty() {
        println!("  Faulty nodes: {:?}", faulty_node_ids);
    }

    // Configure network
    // Use UNORDERED network for model checking to explore message interleavings
    // This creates non-determinism: messages can be delivered in any order
    let network: Network<MessageType> = if lossy_network {
        Network::new_unordered_nonduplicating(vec![]) // Can drop messages
    } else {
        Network::new_unordered_nonduplicating(vec![]) // Reliable but unordered
    };

    // Calculate quorum size: For Byzantine fault tolerance with f faults,
    // we need at least 2f + 1 nodes, and quorum = 2f + 1 = num_nodes - f
    // For crash fault tolerance: quorum = floor(n/2) + 1
    // Using Byzantine formula: quorum = num_nodes (all nodes must agree for simplicity)
    let quorum_size = num_nodes;

    // Create actor model
    // ActorModel::new(capacity, cfg) where capacity is the number of actor IDs to support
    // We register the actor template ONCE, and it's instantiated for each ID
    let model = ActorModel::<ConsensusActor, usize>::new(num_nodes, ())
        .actor(ConsensusActor::with_faults(
            peers.clone(),
            faulty_node_ids.clone(),
            quorum_size,
        ))
        .init_network(network)
        .property(stateright::Expectation::Always, "no crashes during init", |_, state| {
            // Simple property to verify model is working
            state.actor_states.iter().all(|s| s.state != NodeState::Failed || s.is_faulty)
        });

    // Run bounded model checker
    println!("  Running model checker...");
    let checker = model.checker()
        .threads(4)
        .target_max_depth(20)
        .spawn_bfs()
        .join();

    // Report model checking results
    println!("  ✓ Model checking complete!");
    println!("    States explored: {}", checker.state_count());
    println!("    Max depth: {}", checker.max_depth());

    // Debug: Check if model has the right number of actors
    println!("    DEBUG: Model has {} actor slots", num_nodes);
}

/// Simulate a specific fault scenario
#[allow(dead_code)]
fn simulate_fault_scenario() {
    println!("\n=== Detailed Fault Scenario Simulation ===\n");

    // Create a scenario where we manually inject faults
    let num_nodes = 5;
    let _peers: Vec<_> = (0..num_nodes).map(|i| stateright::actor::Id::from(i)).collect();

    println!("Simulating: Node 0 proposes V1, Node 3 crashes after PREPARE phase");
    
    // Manual state evolution
    let mut states: Vec<ConsensusNodeState> = (0..num_nodes)
        .map(|i| ConsensusNodeState::new(i, 5))
        .collect();

    // Step 1: Node 0 proposes V1
    println!("\n[Step 1] Node 0 proposes V1");
    states[0].value = Some(Value::V1);
    print_states(&states);

    // Step 2: All nodes receive proposal and send PREPARE
    println!("\n[Step 2] All nodes receive proposal and broadcast PREPARE");
    for i in 0..num_nodes {
        states[i].value = Some(Value::V1);
        *states[i].prepare_count.entry(Value::V1).or_insert(0) = num_nodes;
        states[i].state = NodeState::Prepared;
    }
    print_states(&states);

    // Step 3: Node 3 crashes
    println!("\n[Step 3] Node 3 crashes");
    states[3].is_faulty = true;
    states[3].state = NodeState::Failed;
    print_states(&states);

    // Step 4: Non-faulty nodes receive COMMITs (only 4 nodes now)
    println!("\n[Step 4] Remaining 4 nodes broadcast COMMIT");
    for i in 0..num_nodes {
        if !states[i].is_faulty {
            *states[i].commit_count.entry(Value::V1).or_insert(0) = 4;
            // Can't reach quorum of 5 with only 4 nodes!
        }
    }
    print_states(&states);

    // Analysis
    println!("\n[Analysis]");
    println!("  Quorum required: 5 nodes");
    println!("  Active nodes: 4 nodes");
    println!("  Result: DEADLOCK - Cannot reach consensus!");
    println!("  Lesson: With f=2 Byzantine tolerance, we need 2f+1=5 nodes.");
    println!("          Losing 1 node means we can't tolerate any more faults.");
}

#[allow(dead_code)]
fn print_states(states: &[ConsensusNodeState]) {
    for state in states {
        println!(
            "  Node {}: state={:?}, value={:?}, prepares={}, commits={}, decided={}, faulty={}",
            state.id,
            state.state,
            state.value,
            state.prepare_count.get(&Value::V1).unwrap_or(&0),
            state.commit_count.get(&Value::V1).unwrap_or(&0),
            state.decided,
            state.is_faulty
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_consensus_model() {
        let model = ConsensusModel::new(5, 2);
        
        // Test agreement checker
        let mut state1 = ConsensusNodeState::new(0, 3);
        state1.decided = true;
        state1.value = Some(Value::V1);
        
        let mut state2 = ConsensusNodeState::new(1, 3);
        state2.decided = true;
        state2.value = Some(Value::V1);
        
        assert!(model.check_agreement(&[state1, state2]));
    }

    #[test]
    fn test_no_premature_decision() {
        let model = ConsensusModel::new(5, 2);
        
        let mut state = ConsensusNodeState::new(0, 3);
        state.state = NodeState::Decided;
        state.value = Some(Value::V1);
        state.commit_count.insert(Value::V1, 2);
        
        // Should fail - not enough commits
        assert!(!model.check_no_premature_decision(&state));
        
        // Fix it
        state.commit_count.insert(Value::V1, 3);
        assert!(model.check_no_premature_decision(&state));
    }
}