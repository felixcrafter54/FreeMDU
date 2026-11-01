# Software ID 266 (tumble dryer) – research notes

Work-in-progress notes for adding support for a Miele tumble dryer reporting software ID 266.
Nothing here is confirmed by firmware analysis yet; all findings come from observing memory
changes with the [`watch_memory`](../protocol/examples/watch_memory.rs) example.

## Diagnostic access

| Key         | Value    |
|-------------|----------|
| Read access | `0x0809` |
| Full access | `0x5804` |

## Memory

- Only the first 256 bytes of memory are accessible. Higher addresses mirror `0x00`–`0xff`,
  so ROM contents could not be dumped this way.
- `0x00`–`0xa1` contain live data, `0xa2`–`0xff` read as `0x00`.
- `0x92`–`0xa1` change constantly and look like return addresses, most likely the stack.

### Noisy addresses

These addresses change continuously without any user interaction (timers, ADC readings)
and can be hidden with `--mute`:

```
0004,0011,0013,0014,0015,0017,0024,0043,004a,0054,0055,0056,0058,0059,005a,0068,0069,006b,006c
```

### Observed addresses

| Address         | Observation                                                                 | Confidence |
|-----------------|-----------------------------------------------------------------------------|------------|
| `0x0016`        | `0x02` when door closed, `0x00` when door open                              | Likely     |
| `0x0020`        | Bit `0x02` set while door open (`0x10` → `0x12`)                            | Likely     |
| `0x0025`        | `0x40` while door open                                                      | Guess      |
| `0x0044`        | `0x02` while door open, `0x01` while program running                        | Guess      |
| `0x004b`        | Operating state: `0x03` ready, `0x0f` running, `0x04` finished, `0x11` door open | Likely |
| `0x005c-0x005d` | Counts up while door is open, reset when closed                             | Guess      |
| `0x0010`        | `0x03` while program running (actuators?)                                   | Guess      |
| `0x001e-0x001f` | `0x02`, `0x08` while program running                                        | Guess      |
| `0x0023`        | Non-zero while program running (`0x24`, `0x34`)                             | Guess      |
| `0x003f`        | Increments while program running                                            | Guess      |
| `0x0051-0x0052` | `0xff` while program running                                                | Guess      |
| `0x005e`        | `0x08` while program running                                                | Guess      |

### EEPROM

The EEPROM is 512 bytes (mirrored 4 times in a 2048 byte dump).

| Address       | Observation                                                              | Confidence |
|---------------|--------------------------------------------------------------------------|------------|
| `0x00`–`0x05` | Same 16-bit value stored 3 times (`0x046d` = 1133), possibly operating hours | Guess  |

## Open questions

- Selected program (program selector position) – not yet observed.
- Program phase, remaining time, dryness level, temperature sensors.
- Operating hours and fault memory (compare with the dryer's service mode).
