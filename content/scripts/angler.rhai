// Wendel sells fishing tackle, and buys fish. (What he says when you talk to
// him is in dialogue/starting_town.yarn; this is what everyone nearby sees.)

fn on_spawn(npc) {
    npc.say("Worms! Fresh worms!");
}

fn on_idle(npc) {
    if chance(0.5) {
        npc.wander();
    } else if chance(0.1) {
        npc.say("The dace are biting on the river today.");
    } else if chance(0.1) {
        npc.say("Fresh bait, fair prices!");
    }
}
