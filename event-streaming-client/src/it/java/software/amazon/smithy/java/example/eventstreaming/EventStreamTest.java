package software.amazon.smithy.java.example.eventstreaming;

import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.Flow;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

import org.junit.jupiter.api.Test;
import software.amazon.smithy.java.client.core.endpoint.EndpointResolver;
import software.amazon.smithy.java.example.eventstreaming.client.FizzBuzzServiceClient;
import software.amazon.smithy.java.example.eventstreaming.model.BuzzEvent;
import software.amazon.smithy.java.example.eventstreaming.model.FizzBuzzInput;
import software.amazon.smithy.java.example.eventstreaming.model.FizzBuzzOutput;
import software.amazon.smithy.java.example.eventstreaming.model.FizzBuzzStream;
import software.amazon.smithy.java.example.eventstreaming.model.FizzEvent;
import software.amazon.smithy.java.example.eventstreaming.model.Value;
import software.amazon.smithy.java.example.eventstreaming.model.ValueStream;
import software.amazon.smithy.java.logging.InternalLogger;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.fail;


// TODO: Update the test to create and run the server in setup before the test
public class EventStreamTest {
    private static final InternalLogger LOGGER = InternalLogger.getLogger(EventStreamTest.class);

    @Test
    public void fizzBuzz() throws InterruptedException {
        var client = FizzBuzzServiceClient.builder()
                .endpointResolver(EndpointResolver.staticHost("http://localhost:9543"))
                .build();

        int range = 100;

        FizzBuzzInput input = FizzBuzzInput.builder()
                .stream(new ValueStreamPublisher(range))
                .build();
        FizzBuzzOutput output = client.fizzBuzz(input);

        LOGGER.info("Initial messages done");

        AtomicLong receivedEvents = new AtomicLong();
        Set<Long> unbuzzed = new HashSet<>();
        AtomicBoolean done = new AtomicBoolean();
        output.getStream().subscribe(new Flow.Subscriber<>() {

            private Flow.Subscription subscription;

            @Override
            public void onSubscribe(Flow.Subscription subscription) {
                this.subscription = subscription;
                subscription.request(1L);
            }

            @Override
            public void onNext(FizzBuzzStream item) {
                receivedEvents.incrementAndGet();
                //LOGGER.info("received: {}", item);
                long value;
                try {
                    switch (item.type()) {
                        case fizz:
                            value = item.<FizzEvent>getValue().getValue();
                            LOGGER.info("received fizz: {}", value);
                            assertEquals(0, value % 3);
                            if (value % 5 == 0) {
                                assertTrue(unbuzzed.add(value), "Fizz already received for " + value);
                            }
                            break;
                        case buzz:
                            value = item.<BuzzEvent>getValue().getValue();
                            LOGGER.info("received buzz: {}", value);
                            assertEquals(0, value % 5);
                            if (value % 3 == 0) {
                                assertTrue(unbuzzed.remove(value), "No fizz for " + value);
                            }
                            break;
                        default:
                            fail("Unexpected event: " + item.type());
                            break;
                    }
                } catch (Exception e) {
                    onError(e);
                }
                subscription.request(1L);
            }

            @Override
            public void onError(Throwable throwable) {
                LOGGER.warn("output stream threw an exception", throwable);
                throwable.printStackTrace();
                done.set(true);
            }

            @Override
            public void onComplete() {
                LOGGER.info("output stream completed");
                done.set(true);
            }
        });

        var waits = 10;
        do {
            Thread.sleep(200);
            --waits;
            if (waits <= 0) {
                throw new RuntimeException("Timed out waiting for completion after " + waits + " waits");
            }
        } while (!done.get());

        if (!unbuzzed.isEmpty()) {
            for (var u : unbuzzed) {
                System.out.println("Unbuzzed: " + u);
            }
        }
        assertTrue(unbuzzed.isEmpty(), unbuzzed.size() + " unbuzzed fizzes");
        assertEquals((range / 3) + (range / 5), receivedEvents.get());
    }

    private static class ValueStreamPublisher implements Flow.Publisher<ValueStream> {
        private final int range;

        public ValueStreamPublisher(int range) {
            this.range = range;
        }

        @Override
        public void subscribe(Flow.Subscriber<? super ValueStream> subscriber) {

            subscriber.onSubscribe(new Flow.Subscription() {

                int count = 0;

                @Override
                public void request(long n) {
                    // sleeping between sending request events so there's chance to process response events in parallel
                    try {
                        Thread.sleep(20);
                    } catch (InterruptedException e) {
                        throw new RuntimeException(e);
                    }
                    if (count++ < range) {
                        ValueStream value = ValueStream.builder()
                                .value(Value.builder().value(count).build())
                                .build();
                        //LOGGER.info("Publishing value {}", value);
                        subscriber.onNext(value);
                    } else {
                        subscriber.onComplete();
                    }
                }

                @Override
                public void cancel() {
                    subscriber.onComplete();
                }
            });
        }
    }
}
