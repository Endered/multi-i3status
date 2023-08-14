# multi-i3status
multiplexer for i3status

# usage
```bash
i3status | multi-i3status both 0 2.0

# output remote server's i3status instead of local i3status.
ssh "SOME SSH SERVER" i3status | multi-i3status reader 1
```

## how to use

### reader
```bash
# read i3status and output to fifo file with rank
i3status | multi-i3status reader [rank]
```

### reciever
```bash
# read from fifo file and output in i3status format
# output highest rank input only
multi-i3status reciever [duration]
```

### both
```bash
# work with both reader and reciever
i3status | multi-i3status [rank] [duration]
```

### simple

```bash
i3status | multi-i3status reader
multi-i3status reciever 

```

### multiplexer

```bash
# (a) read i3status and output to fifo file with rank 0
i3status | multi-i3status reader 0

# (b) read i3status and output to fifo file with rank 1
i3status | multi-i3status reader 1

# read from fifo file and output in i3status format
# But, output only (b)
multi-i3status reciever
```
