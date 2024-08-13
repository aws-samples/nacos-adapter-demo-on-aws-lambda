package helloworld;

import com.alibaba.nacos.api.NacosFactory;
import com.alibaba.nacos.api.config.ConfigService;
import com.alibaba.nacos.api.exception.NacosException;
import com.amazonaws.services.lambda.runtime.Context;
import com.amazonaws.services.lambda.runtime.RequestHandler;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyRequestEvent;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyResponseEvent;

import software.amazon.cloudwatchlogs.emf.logger.MetricsLogger;
import software.amazon.cloudwatchlogs.emf.model.Unit;
import software.amazon.lambda.powertools.metrics.Metrics;
import software.amazon.lambda.powertools.metrics.MetricsUtils;

public class App implements RequestHandler<APIGatewayProxyRequestEvent, APIGatewayProxyResponseEvent> {
  ConfigService configService;
  MetricsLogger metricsLogger = MetricsUtils.metricsLogger();

  public App() throws NacosException {
    configService = NacosFactory.createConfigService("localhost:8848");
  }

  @Metrics(namespace = "NacosAdapterTest", service = "simple-java")
  public APIGatewayProxyResponseEvent handleRequest(final APIGatewayProxyRequestEvent input, final Context context) {
    var response = new APIGatewayProxyResponseEvent();

    try {
      var start = System.currentTimeMillis();
      final var config = getConfig();
      var elapsed = System.currentTimeMillis() - start;

      metricsLogger.putMetric("GetConfigLatency", elapsed, Unit.MILLISECONDS);

      return response
          .withStatusCode(200)
          .withBody(config);
    } catch (NacosException e) {
      return response
          .withBody("error")
          .withStatusCode(500);
    }
  }

  String getConfig() throws NacosException {
    return configService.getConfig("test", "DEFAULT_GROUP", 5000);
  }
}
