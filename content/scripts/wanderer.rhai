// Old Tomas ambles about the square, muttering to himself. (What he says when
// you talk to him is in dialogue/starting_town.yarn; this is what everyone
// nearby sees.)

fn on_idle(npc) {
    npc.wander();
    if chance(0.1) {
        npc.say("Hmm... where did I leave my pipe?");
    }
}
