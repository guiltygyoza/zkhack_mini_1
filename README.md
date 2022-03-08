late(!) submission: zkhack-there-is-something-in-the-AIR
-------------------
Collaborators: Austin Liu, Ole Spjeldnæs 

Idea
==================
1. Discovered missing boundary constraints for capacity of Rescue hash (in `lib.rs`)
2. Changed in `prover.rs` the initial value for register #12 from 8 to 1
3. Copied Rescue::merge() from official Winterfell to puzzle repo, changed the capacity initial value from (8,0,0,0) to (1,0,0,0), and used this "evil" merge function to calculate nullifer, which is used as public input to the execution trace.

Puzzle description
==================

```
    ______ _   __  _   _            _
    |___  /| | / / | | | |          | |
       / / | |/ /  | |_| | __ _  ___| | __
      / /  |    \  |  _  |/ _` |/ __| |/ /
    ./ /___| |\  \ | | | | (_| | (__|   <
    \_____/\_| \_/ \_| |_/\__,_|\___|_|\_\

Alice implemented a Semaphore protocol to collect anonymous votes from her friends on various
topics. She collected public keys from 7 of her friends, and together with her public key, built
an access set out of them.

During one of the votes, Alice collected 9 valid signals on the same topic. But that should not be
possible! The semaphore protocol guarantees that every user can vote only once on a given topic.
Someone must have figured out how to create multiple signals on the same topic.

Below is a transcript for generating a valid signal on a topic using your private key. Can you
figure out how to create a valid signal with a different nullifier on the same topic?
```
