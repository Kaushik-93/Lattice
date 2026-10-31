# Prompts

LLM prompts live here as versioned files, never as inline strings (spec §11). They ship
inside the wheel so a deployed service always runs the prompts it was tested with.

Convention, starting in phase 4:

```
prompts/
  ask/
    system.v1.md          # one file per version; never edit a released version in place
    system.v2.md
  propose_logic/
    system.v1.md
```

The agent loads prompts by `(name, version)`; the version in use is recorded on every
Ask trace and in the eval results so accuracy changes can be attributed to prompt changes.
