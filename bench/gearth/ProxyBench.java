import gearth.app.protocol.HConnection;
import gearth.protocol.connection.HClient;
import gearth.app.protocol.connection.HProxy;
import gearth.app.protocol.connection.HState;
import gearth.app.protocol.connection.proxy.flash.FlashProxyProvider;
import gearth.app.services.extension_handler.ExtensionHandler;

import javafx.application.Platform;

import java.lang.reflect.Method;
import java.util.concurrent.CountDownLatch;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;

public class ProxyBench {
    static class HarnessProvider extends FlashProxyProvider {
        HarnessProvider(HConnection connection) {
            super(proxy -> {}, state -> {}, connection);
        }

        void bridge(Socket client, Socket server, HProxy proxy) throws Exception {
            startProxyThread(client, server, proxy);
        }

        @Override
        public void start() {}

        @Override
        public void abort() {}

        @Override
        protected void onConnect() {}

        @Override
        protected void onConnectEnd() {}
    }

    public static void main(String[] args) throws Exception {
        int listenPort = Integer.parseInt(args[0]);
        int hotelPort = Integer.parseInt(args[1]);
        HConnection connection = new HConnection();
        CountDownLatch ready = new CountDownLatch(1);
        Platform.startup(() -> {
            new ExtensionHandler(connection);
            ready.countDown();
        });
        ready.await();
        Method setState = HConnection.class.getDeclaredMethod("setState", HState.class);
        setState.setAccessible(true);
        setState.invoke(connection, HState.WAITING_FOR_CLIENT);
        HarnessProvider provider = new HarnessProvider(connection);
        try (ServerSocket listener = new ServerSocket(listenPort, 10, InetAddress.getLoopbackAddress())) {
            System.out.println("gearth proxy listening on " + listenPort);
            while (true) {
                Socket client = listener.accept();
                Socket server = new Socket(InetAddress.getLoopbackAddress(), hotelPort);
                HProxy proxy = new HProxy(HClient.FLASH, "127.0.0.1", "127.0.0.1", hotelPort, listenPort, "127.0.0.1");
                new Thread(() -> {
                    try {
                        provider.bridge(client, server, proxy);
                    } catch (Exception e) {
                        e.printStackTrace();
                    }
                }).start();
            }
        }
    }
}
