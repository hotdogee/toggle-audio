# Generated summary (2026-10-04 20:30:49 +08:00)

mean ± σ (median), ms; hyperfine -N, pooled over rounds.

| Implementation | Size (bytes) | noargs | list | get | set-noop | toggle |
| --- | ---: | --- | --- | --- | --- | --- |
| nop | 2,560 | 3.45 ± 0.49 (3.37) | n/a | n/a | n/a | n/a |
| nop-con | 2,560 | 3.53 ± 0.47 (3.43) | n/a | n/a | n/a | n/a |
| nop-con-detached | 4,096 | 3.47 ± 0.53 (3.39) | n/a | n/a | n/a | n/a |
| ta-c | 116,224 | 7.52 ± 0.80 (7.38) | 17.57 ± 1.01 (17.36) | 13.94 ± 0.76 (13.81) | 38.25 ± 2.52 (37.81) | 41.83 ± 2.83 (41.59) |
| ta-c-gui | 116,224 | 7.60 ± 0.64 (7.52) | 17.67 ± 1.44 (17.39) | 13.81 ± 0.83 (13.62) | 39.18 ± 3.51 (38.54) | 40.59 ± 1.90 (40.58) |
| ta-c-md | 23,040 | 8.04 ± 0.81 (7.88) | 17.37 ± 0.96 (17.13) | 14.00 ± 0.98 (13.80) | 38.70 ± 2.84 (38.27) | 40.68 ± 1.95 (40.90) |
| ta-c-delayload | 119,296 | 4.24 ± 0.60 (4.06) | 17.34 ± 1.11 (17.17) | 13.70 ± 0.76 (13.53) | 38.45 ± 2.65 (38.10) | 42.75 ± 4.44 (41.54) |
| ta-c-nocrt | 20,480 | 7.06 ± 0.78 (6.96) | 17.30 ± 1.09 (17.08) | 13.47 ± 0.78 (13.31) | 38.37 ± 2.73 (37.93) | 40.01 ± 1.92 (40.17) |
| ta-rs | 250,368 | 8.08 ± 0.90 (7.96) | 17.19 ± 1.03 (16.99) | 13.57 ± 0.72 (13.40) | 38.87 ± 2.83 (38.54) | 43.79 ± 2.74 (43.52) |
| ta-cs | 1,001,984 | 11.83 ± 1.27 (11.63) | 18.95 ± 1.15 (18.71) | 14.87 ± 0.80 (14.73) | 39.25 ± 2.67 (38.91) | 43.59 ± 3.63 (42.78) |
| ta-go | 1,530,368 | 6.07 ± 0.64 (5.93) | 19.94 ± 1.62 (19.52) | 15.36 ± 0.80 (15.15) | 40.34 ± 2.70 (39.93) | 42.30 ± 2.64 (41.56) |
| ta-zig | 16,384 | 7.05 ± 0.53 (6.97) | 17.78 ± 1.32 (17.52) | 13.50 ± 0.81 (13.31) | 38.90 ± 3.42 (38.24) | 40.87 ± 2.43 (40.81) |
| ta-zigcc | 88,064 | 7.58 ± 0.51 (7.51) | 18.27 ± 1.94 (17.81) | 13.60 ± 0.69 (13.47) | 38.32 ± 2.54 (37.93) | 40.92 ± 2.36 (41.05) |
| toggle-audio | 521,216 | 9.78 ± 0.78 (9.67) | 19.74 ± 1.24 (19.43) | 15.51 ± 0.85 (15.32) | 19.54 ± 1.07 (19.38) | 40.91 ± 4.13 (40.05) |
| toggle-audiow | 521,216 | 9.76 ± 0.67 (9.64) | 19.79 ± 1.27 (19.48) | 15.41 ± 0.88 (15.15) | 19.71 ± 1.10 (19.54) | 42.63 ± 2.60 (42.17) |

| PowerShell row | Scenario | mean ± σ (median) |
| --- | --- | --- |
| ps-host-empty | ps-noargs | 132.98 ± 3.67 (132.43) |
| pwsh-host-empty | ps-noargs | 198.80 ± 5.52 (197.66) |
| ps-ta (usage) | ps-noargs | 200.96 ± 2.89 (200.23) |
| pwsh-ta (usage) | ps-noargs | 358.29 ± 9.35 (354.43) |
| ta-ps (usage) | ps-noargs | 207.12 ± 4.74 (206.07) |
| ps-ta list | ps-list | 519.83 ± 9.01 (517.35) |
| pwsh-ta list | ps-list | 732.70 ± 10.33 (731.54) |
| ta-ps list | ps-list | 551.07 ± 9.79 (546.25) |
| ps-ta set-noop | ps-set-noop | 266.96 ± 12.31 (263.79) |
| pwsh-ta set-noop | ps-set-noop | 419.34 ± 6.30 (418.59) |
| ta-ps set-noop | ps-set-noop | 278.01 ± 30.97 (273.50) |
| ta-ps-anycpu set-noop | ps-set-noop | 279.80 ± 11.14 (277.36) |
| ps-ta | toggle | 289.49 ± 6.34 (289.10) |
| pwsh-ta | toggle | 437.97 ± 7.74 (435.32) |
| Switch-Audio (legacy) | toggle | 887.54 ± 10.16 (887.38) |
| switch-audio.ps1 (legacy) | toggle | 858.50 ± 10.26 (858.05) |
| ta-ps | toggle | 319.11 ± 10.29 (322.17) |

Phase medians (µs since process creation), set <PG42UQ> --timing:

- ta-c : entry=6044, com_init=8107.1, enumerator=10266.2, work_done=36727.4, exit=37784
- ta-c-gui : entry=6092.6, com_init=8139.2, enumerator=10467.8, work_done=35355.2, exit=36425.1
- ta-c-md : entry=6013.2, com_init=8023.8, enumerator=10283.1, work_done=35281.4, exit=36242.2
- ta-c-delayload : entry=3121, com_init=7653.5, enumerator=9771.2, work_done=33450.6, exit=34408.3
- ta-c-nocrt : entry=5720.9, com_init=7598.8, enumerator=9682.2, work_done=34417.5, exit=35497.7
- ta-rs : entry=5692, com_init=7669.2, enumerator=9765.6, work_done=34037.4, exit=35147.8
- ta-cs : entry=8498.2, com_init=8501.1, enumerator=10853.6, work_done=33992.6, exit=33996.6
- ta-go : entry=5071.4, com_init=9029.4, enumerator=11224.2, work_done=35005.2, exit=36046.4
- ta-zig : entry=5835.7, com_init=7715.8, enumerator=9919.2, work_done=35295.6, exit=36442.7
- ta-zigcc : entry=5671.8, com_init=7568.2, enumerator=9777.2, work_done=32812.2, exit=33821.6
- toggle-audio : start=6900.8, config_loaded=6995.6, com_ready=10595.9, target_chosen=10841.2, set_done=14842.6, end=15283.4
- toggle-audiow : start=6892, config_loaded=6988.1, com_ready=10596, target_chosen=10853.3, set_done=14873.3, end=15285.2
- ta.ps1 (powershell.exe) : entry=134566.5, com_init=194916.2, enumerator=197107.6, work_done=243362, exit=248640.9
- ta.ps1 (pwsh) : entry=194988.8, com_init=318773, enumerator=321578.6, work_done=364051.1, exit=369094.7
- ta-ps : entry=159069, com_init=206591.8, enumerator=208937.8, work_done=254051, exit=259827.6
