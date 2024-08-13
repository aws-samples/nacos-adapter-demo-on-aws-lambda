package helloworld;

import java.util.Properties;

import com.alibaba.nacos.api.NacosFactory;
import com.alibaba.nacos.api.config.ConfigService;
import com.alibaba.nacos.api.exception.NacosException;
import com.amazonaws.services.lambda.runtime.Context;
import com.amazonaws.services.lambda.runtime.RequestHandler;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyRequestEvent;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyResponseEvent;

public class App implements RequestHandler<APIGatewayProxyRequestEvent, APIGatewayProxyResponseEvent> {
  ConfigService configService;

  public App() throws NacosException {
    var properties = new Properties();
    properties.put("serverAddr", "localhost:8848");
    this.configService = NacosFactory.createConfigService(properties);
  }

  public APIGatewayProxyResponseEvent handleRequest(final APIGatewayProxyRequestEvent input, final Context context) {
    var response = new APIGatewayProxyResponseEvent();

    try {
      final var config = this.getConfig();

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
    return this.configService.getConfig("test", "DEFAULT_GROUP", 5000);
  }
}
