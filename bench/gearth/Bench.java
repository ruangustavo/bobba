import gearth.app.protocol.crypto.RC4;
import gearth.app.protocol.packethandler.flash.FlashBuffer;
import gearth.protocol.HPacket;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

public class Bench {
    static byte[] unhex(String text) {
        byte[] out = new byte[text.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(text.substring(i * 2, i * 2 + 2), 16);
        }
        return out;
    }

    record Recovery(byte[] encrypted, List<byte[]> tables) {}

    static Recovery recovery(Path path) throws Exception {
        List<String> lines = Files.readAllLines(path);
        List<byte[]> tables = new ArrayList<>();
        for (String line : lines.subList(1, lines.size())) {
            if (line.length() == 2048) {
                tables.add(unhex(line));
            }
        }
        return new Recovery(unhex(lines.get(0)), tables);
    }

    static boolean bruteFlash(byte[] encBuffer, byte[] tableState, int q, int j) {
        byte[] keycpy = Arrays.copyOf(tableState, tableState.length);
        RC4 rc4Tryout = new RC4(keycpy, q, j);
        rc4Tryout.undoRc4(encBuffer);
        if (rc4Tryout.couldBeFresh()) {
            byte[] encDataCopy = Arrays.copyOf(encBuffer, encBuffer.length);
            RC4 rc4TryCopy = rc4Tryout.deepCopy();
            try {
                FlashBuffer payloadBuffer = new FlashBuffer();
                byte[] decoded = rc4TryCopy.cipher(encDataCopy);
                payloadBuffer.push(decoded);
                payloadBuffer.receive();
                if (payloadBuffer.isEmpty()) {
                    found = rc4Tryout;
                    return true;
                }
                return false;
            } catch (Exception e) {
                return false;
            }
        }
        return false;
    }

    static RC4 found;

    static boolean obtain(Recovery input) {
        int estimatedQ = input.encrypted().length % 256;
        for (byte[] table : input.tables()) {
            for (int j = 0; j < 256; j++) {
                if (bruteFlash(input.encrypted(), table, estimatedQ, j)) return true;
            }
        }
        for (byte[] table : input.tables()) {
            for (int q = 0; q < 256; q++) {
                for (int j = 0; j < 256; j++) {
                    if (bruteFlash(input.encrypted(), table, q, j)) return true;
                }
            }
        }
        return false;
    }

    static volatile Object sink;

    static void measure(String name, int warmup, int rounds, int items, Runnable run) {
        for (int i = 0; i < warmup; i++) run.run();
        long[] samples = new long[rounds];
        for (int i = 0; i < rounds; i++) {
            long started = System.nanoTime();
            run.run();
            samples[i] = System.nanoTime() - started;
        }
        Arrays.sort(samples);
        double median = samples[rounds / 2] / 1e6;
        double best = samples[0] / 1e6;
        System.out.printf("%-34s median %10.3f ms  best %10.3f ms  per item %9.3f µs  (%d rounds)%n",
                name, median, best, median * 1000 / Math.max(1, items), rounds);
    }

    public static void main(String[] args) throws Exception {
        Path dir = Path.of(args.length > 0 ? args[0] : "bench/inputs");
        Recovery success = recovery(dir.resolve("recovery_success.txt"));
        Recovery failure = recovery(dir.resolve("recovery_failure.txt"));
        System.out.println("gearth: success input recovered = " + obtain(success));
        byte[] expected = unhex(Files.readAllLines(dir.resolve("expected_fresh.txt")).get(0));
        int[] state = found.getState();
        boolean right = true;
        for (int i = 0; i < 256; i++) {
            right &= (state[i] & 0xff) == (expected[i * 4 + 3] & 0xff);
        }
        System.out.println("gearth: picked the right key = " + right);

        measure("key recovery (key present)", 3, 20, 1, () -> sink = obtain(success));
        measure("key recovery (no key, gives up)", 1, 3, 1, () -> sink = obtain(failure));

        List<String> expressions = Files.readAllLines(dir.resolve("expressions.txt"));
        List<byte[]> packets = new ArrayList<>();
        for (String line : Files.readAllLines(dir.resolve("packets.hex"))) {
            if (!line.isEmpty()) packets.add(unhex(line));
        }
        measure("parse 10k expressions", 5, 30, expressions.size(), () -> {
            for (String expression : expressions) sink = new HPacket(expression);
        });
        measure("predict+format 10k expressions", 5, 30, packets.size(), () -> {
            for (byte[] packet : packets) sink = new HPacket(packet).toExpression();
        });
    }
}
